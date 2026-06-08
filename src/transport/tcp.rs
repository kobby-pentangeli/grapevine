//! TCP transport implementation.

use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use dashmap::DashMap;
use futures::SinkExt;
use futures::stream::StreamExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::{AbortHandle, JoinHandle};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{debug, error, warn};

use crate::core::message_codec::MAX_FRAME_SIZE;
use crate::{Error, Message, MessageCodec, Peer, PeerInfo, RateLimiter, Result};

const WRITE_CHANNEL_CAPACITY: usize = 1024;
const RECV_CHANNEL_CAPACITY: usize = 1024;
const SHUTDOWN_DRAIN_GRACE_MS: u64 = 500;

struct ConnectionTask {
    supervisor: JoinHandle<()>,
    read_abort: AbortHandle,
    write_abort: AbortHandle,
}

/// TCP transport for gossip messages.
pub struct Tcp {
    /// Local listening address, set once when the transport begins listening.
    local_addr: OnceLock<SocketAddr>,

    /// Active peer connections
    peers: Arc<DashMap<SocketAddr, Peer>>,

    /// Per-connection task handles, keyed by connection address.
    connections: Arc<DashMap<SocketAddr, ConnectionTask>>,

    /// Channel for receiving messages from all peers
    message_rx: Arc<Mutex<Receiver<(SocketAddr, Message)>>>,

    /// Channel for sending messages (cloned to connection handlers)
    message_tx: Sender<(SocketAddr, Message)>,

    /// Rate limiting configuration
    rate_limiter: Option<Arc<Mutex<RateLimiter>>>,

    /// Maximum message size in bytes
    max_message_size: usize,

    /// Maximum number of simultaneous peer connections
    max_peers: usize,

    /// Accept-loop task handle, set once when the transport begins listening.
    accept_handle: Mutex<Option<JoinHandle<()>>>,
}

impl Tcp {
    /// Create a new TCP transport with default settings.
    pub fn new() -> Self {
        Self::with_max_message_size(MAX_FRAME_SIZE) // 10 MB default
    }

    /// Create a new TCP transport with specified max message size.
    pub fn with_max_message_size(max_message_size: usize) -> Self {
        let (message_tx, message_rx) = mpsc::channel(RECV_CHANNEL_CAPACITY);

        Self {
            local_addr: OnceLock::new(),
            peers: Arc::new(DashMap::new()),
            connections: Arc::new(DashMap::new()),
            message_rx: Arc::new(Mutex::new(message_rx)),
            message_tx,
            rate_limiter: None,
            max_message_size,
            max_peers: usize::MAX,
            accept_handle: Mutex::new(None),
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

    /// Cap the number of simultaneous peer connections.
    ///
    /// Connections beyond `max_peers` are refused on both the inbound (accept)
    /// and outbound (connect) paths. The default is unbounded.
    pub fn set_max_peers(mut self, max_peers: usize) -> Self {
        self.max_peers = max_peers;
        self
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
        let connections = Arc::clone(&self.connections);
        let message_tx = self.message_tx.clone();
        let rate_limiter = self.rate_limiter.clone();
        let max_message_size = self.max_message_size;
        let max_peers = self.max_peers;

        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        if peers.len() >= max_peers {
                            debug!("At max_peers ({max_peers}), refusing inbound from {peer_addr}");
                            continue;
                        }
                        debug!("Accepted connection from {peer_addr}");
                        Self::handle_connection(
                            stream,
                            peer_addr,
                            Arc::clone(&peers),
                            Arc::clone(&connections),
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

        *self.accept_handle.lock().await = Some(handle);

        Ok(())
    }

    /// Connect to a peer.
    pub async fn connect(&self, addr: SocketAddr) -> Result<()> {
        if self.local_addr() == Some(addr) {
            return Err(Error::network(format!(
                "refusing self-connection to {addr}"
            )));
        }
        if self.peers.contains_key(&addr) {
            debug!("Already connected to {addr}");
            return Ok(());
        }
        if self.peers.len() >= self.max_peers {
            return Err(Error::network(format!(
                "at max_peers ({}), refusing connection to {addr}",
                self.max_peers
            )));
        }

        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| Error::Connection { addr, source: e })?;

        debug!("TCP connection established to {addr}");

        Self::handle_connection(
            stream,
            addr,
            Arc::clone(&self.peers),
            Arc::clone(&self.connections),
            self.message_tx.clone(),
            self.rate_limiter.clone(),
            self.max_message_size,
        );

        Ok(())
    }

    /// Queue a message for delivery to a peer.
    ///
    /// The message is encoded exactly once, by the peer's writer task at the
    /// socket; this only enqueues it. Returns [`Error::PeerNotFound`] if the
    /// peer is not connected. A full write channel drops the frame rather than
    /// erroring (see [`Peer::send`]).
    pub async fn send(&self, peer: SocketAddr, message: Message) -> Result<()> {
        if let Some(mut conn) = self.peers.get_mut(&peer) {
            conn.send(message)
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

    /// The maximum serialized frame size, in bytes, this transport will emit or
    /// accept. Repair logic uses it to keep anti-entropy responses deliverable.
    pub fn max_message_size(&self) -> usize {
        self.max_message_size
    }

    /// Get list of connected peers.
    pub fn peers(&self) -> Vec<SocketAddr> {
        self.peers.iter().map(|entry| *entry.key()).collect()
    }

    /// Snapshot every connected peer's address and current [`PeerInfo`].
    pub fn peer_infos(&self) -> Vec<(SocketAddr, PeerInfo)> {
        self.peers
            .iter()
            .map(|entry| (*entry.key(), entry.value().info.clone()))
            .collect()
    }

    /// Mark a connected peer as stale.
    pub fn mark_stale(&self, addr: SocketAddr) {
        if let Some(mut peer) = self.peers.get_mut(&addr) {
            peer.info.mark_stale();
        }
    }

    /// Drop a peer from the registry, returning whether it was present.
    pub fn disconnect(&self, addr: SocketAddr) -> bool {
        let removed = self.peers.remove(&addr).is_some();
        if let Some((_, conn)) = self.connections.remove(&addr) {
            conn.read_abort.abort();
            conn.write_abort.abort();
        }
        removed
    }

    /// Stop the transport.
    pub async fn shutdown(&self) {
        if let Some(handle) = self.accept_handle.lock().await.take() {
            handle.abort();
            let _ = handle.await;
        }

        for entry in self.connections.iter() {
            entry.value().read_abort.abort();
        }

        self.peers.clear();

        let drained: Vec<ConnectionTask> = {
            let addrs: Vec<SocketAddr> = self.connections.iter().map(|e| *e.key()).collect();
            addrs
                .into_iter()
                .filter_map(|addr| self.connections.remove(&addr).map(|(_, conn)| conn))
                .collect()
        };

        let (supervisors, write_aborts): (Vec<_>, Vec<_>) = drained
            .into_iter()
            .map(|conn| (conn.supervisor, conn.write_abort))
            .unzip();

        let reap = futures::future::join_all(supervisors);
        if tokio::time::timeout(Duration::from_millis(SHUTDOWN_DRAIN_GRACE_MS), reap)
            .await
            .is_err()
        {
            for abort in &write_aborts {
                abort.abort();
            }
        }
    }

    fn handle_connection(
        stream: TcpStream,
        peer_addr: SocketAddr,
        peers: Arc<DashMap<SocketAddr, Peer>>,
        connections: Arc<DashMap<SocketAddr, ConnectionTask>>,
        message_tx: Sender<(SocketAddr, Message)>,
        rate_limiter: Option<Arc<Mutex<RateLimiter>>>,
        max_message_size: usize,
    ) {
        let (reader, writer) = stream.into_split();
        let (tx, mut rx) = mpsc::channel::<Message>(WRITE_CHANNEL_CAPACITY);

        peers.insert(peer_addr, Peer::new(peer_addr, tx));

        let codec = MessageCodec::with_max_frame_size(max_message_size);

        let write_task = {
            let mut sink = FramedWrite::new(writer, codec.clone());
            tokio::spawn(async move {
                while let Some(message) = rx.recv().await {
                    if let Err(e) = sink.send(message).await {
                        debug!("Failed to send to {peer_addr}: {e}");
                        break;
                    }
                }
                let _ = sink.flush().await;
            })
        };

        let read_task = {
            let read_peers = Arc::clone(&peers);
            let mut stream = FramedRead::new(reader, codec);
            tokio::spawn(async move {
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(message) => {
                            if let Some(mut peer) = read_peers.get_mut(&peer_addr) {
                                peer.info.increment_received();
                            }

                            if let Some(ref limiter) = rate_limiter
                                && !limiter.lock().await.allow_request(peer_addr)
                            {
                                warn!("Rate limit exceeded for peer {peer_addr}, dropping message");
                                continue;
                            }

                            if message_tx.send((peer_addr, message)).await.is_err() {
                                warn!("Inbound channel closed");
                                break;
                            }
                        }
                        Err(e) => {
                            debug!("Connection from {peer_addr} closed: {e}");
                            break;
                        }
                    }
                }
            })
        };

        let read_abort = read_task.abort_handle();
        let write_abort = write_task.abort_handle();

        let supervisor = {
            let peers = Arc::clone(&peers);
            let connections = Arc::clone(&connections);
            tokio::spawn(async move {
                let _ = read_task.await;
                peers.remove(&peer_addr);
                let _ = write_task.await;
                connections.remove(&peer_addr);
                debug!("Connection closed: {peer_addr}");
            })
        };

        connections.insert(
            peer_addr,
            ConnectionTask {
                supervisor,
                read_abort,
                write_abort,
            },
        );
    }
}

impl Default for Tcp {
    fn default() -> Self {
        Self::new()
    }
}
