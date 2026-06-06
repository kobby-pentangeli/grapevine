//! TCP transport implementation.

use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use dashmap::DashMap;
use futures::stream::StreamExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{debug, error, warn};

use crate::core::message_codec::MAX_FRAME_SIZE;
use crate::{Error, Message, MessageCodec, Peer, PeerId, RateLimiter, Result};

/// Maximum attempts to wait for peer registration after connection.
const PEER_REGISTRATION_MAX_ATTEMPTS: u32 = 50;

/// Delay between peer registration checks (milliseconds).
const PEER_REGISTRATION_CHECK_DELAY_MS: u64 = 10;

/// TCP transport for gossip messages.
pub struct Tcp {
    /// Local listening address, set once when the transport begins listening.
    local_addr: OnceLock<SocketAddr>,

    /// Active peer connections
    peers: Arc<DashMap<SocketAddr, Peer>>,

    /// Channel for receiving messages from all peers
    message_rx: Arc<Mutex<UnboundedReceiver<(SocketAddr, Message)>>>,

    /// Channel for sending messages (cloned to connection handlers)
    message_tx: UnboundedSender<(SocketAddr, Message)>,

    /// Rate limiting configuration
    rate_limiter: Option<Arc<Mutex<RateLimiter>>>,

    /// Maximum message size in bytes
    max_message_size: usize,
}

impl Tcp {
    /// Create a new TCP transport with default settings.
    pub fn new() -> Self {
        Self::with_max_message_size(MAX_FRAME_SIZE) // 10 MB default
    }

    /// Create a new TCP transport with specified max message size.
    pub fn with_max_message_size(max_message_size: usize) -> Self {
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        Self {
            local_addr: OnceLock::new(),
            peers: Arc::new(DashMap::new()),
            message_rx: Arc::new(Mutex::new(message_rx)),
            message_tx,
            rate_limiter: None,
            max_message_size,
        }
    }

    /// Enable rate limiting with the given configuration.
    ///
    /// # Errors
    /// Returns [`Error::Config`] if `capacity` or `refill_rate` is zero.
    pub fn set_rate_limit(mut self, capacity: u32, refill_rate: u32) -> Result<Self> {
        self.rate_limiter = Some(Arc::new(Mutex::new(RateLimiter::try_with_params(
            capacity,
            refill_rate,
        )?)));
        Ok(self)
    }

    /// Start listening on the given address.
    pub async fn listen(&self, addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Error::Connection { addr, source: e })?;

        let local_addr = listener.local_addr().map_err(Error::Io)?;
        let _ = self.local_addr.set(local_addr);

        debug!("TCP transport listening on {local_addr}");

        let peers = Arc::clone(&self.peers);
        let message_tx = self.message_tx.clone();
        let rate_limiter = self.rate_limiter.clone();
        let max_message_size = self.max_message_size;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        debug!("Accepted connection from {peer_addr}");
                        Self::handle_connection(
                            stream,
                            peer_addr,
                            Arc::clone(&peers),
                            message_tx.clone(),
                            rate_limiter.clone(),
                            max_message_size,
                        );
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {e}");
                    }
                }
            }
        });

        Ok(())
    }

    /// Connect to a peer.
    pub async fn connect(&self, addr: SocketAddr) -> Result<()> {
        if self.peers.contains_key(&addr) {
            debug!("Already connected to {addr}");
            return Ok(());
        }

        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| Error::Connection { addr, source: e })?;

        debug!("TCP connection established to {addr}");

        Self::handle_connection(
            stream,
            addr,
            Arc::clone(&self.peers),
            self.message_tx.clone(),
            self.rate_limiter.clone(),
            self.max_message_size,
        );

        for _ in 0..PEER_REGISTRATION_MAX_ATTEMPTS {
            if self.peers.contains_key(&addr) {
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(
                PEER_REGISTRATION_CHECK_DELAY_MS,
            ))
            .await;
        }

        Err(Error::Connection {
            addr,
            source: std::io::Error::new(std::io::ErrorKind::TimedOut, "peer registration timeout"),
        })
    }

    /// Send a message to a peer.
    pub async fn send(&self, peer: SocketAddr, message: Message) -> Result<()> {
        let data = bincode::serde::encode_to_vec(&message, bincode::config::standard())?;

        if let Some(conn) = self.peers.get_mut(&peer).as_deref_mut() {
            conn.send(Bytes::from(data))
                .map_err(|err| Error::Channel(err.to_string()))?;
            Ok(())
        } else {
            Err(Error::PeerNotFound(peer))
        }
    }

    /// Receive a message from any peer.
    pub async fn recv(&self) -> Result<(SocketAddr, Message)> {
        self.message_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or(Error::Channel("Channel recv error".to_string()))
    }

    /// Get local listening address.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr.get().copied()
    }

    /// Get list of connected peers.
    pub fn peers(&self) -> Vec<SocketAddr> {
        self.peers.iter().map(|entry| *entry.key()).collect()
    }

    fn handle_connection(
        stream: TcpStream,
        peer_addr: SocketAddr,
        peers: Arc<DashMap<SocketAddr, Peer>>,
        message_tx: UnboundedSender<(SocketAddr, Message)>,
        rate_limiter: Option<Arc<Mutex<RateLimiter>>>,
        max_message_size: usize,
    ) {
        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();

            let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();

            let peer = Peer::new(PeerId(peer_addr), tx);
            peers.insert(peer_addr, peer);

            let codec = MessageCodec::with_max_frame_size(max_message_size);

            // Spawn writer task
            let write_task = {
                let mut sink = FramedWrite::new(writer, codec.clone());
                tokio::spawn(async move {
                    while let Some(data) = rx.recv().await {
                        match bincode::serde::decode_from_slice::<Message, _>(
                            &data,
                            bincode::config::standard(),
                        ) {
                            Ok((message, _)) => {
                                if let Err(e) = futures::SinkExt::send(&mut sink, message).await {
                                    error!("Failed to send to {peer_addr}: {e}");
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to deserialize outgoing message: {e}");
                            }
                        }
                    }
                })
            };

            // Spawn reader task
            let read_task = {
                let mut stream = FramedRead::new(reader, codec);
                tokio::spawn(async move {
                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(message) => {
                                if let Some(ref limiter) = rate_limiter {
                                    let allowed = limiter.lock().await.allow_request(peer_addr);
                                    if !allowed {
                                        warn!(
                                            "Rate limit exceeded for peer {peer_addr}, dropping message"
                                        );
                                        continue;
                                    }
                                }

                                if message_tx.send((peer_addr, message)).is_err() {
                                    warn!("Message channel closed");
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Error reading from {peer_addr}: {e}");
                                break;
                            }
                        }
                    }
                })
            };

            // Wait for either task to complete
            tokio::select! {
                _ = read_task => {},
                _ = write_task => {},
            }

            // Cleanup
            peers.remove(&peer_addr);
            debug!("Connection closed: {peer_addr}");
        });
    }
}

impl Default for Tcp {
    fn default() -> Self {
        Self::new()
    }
}
