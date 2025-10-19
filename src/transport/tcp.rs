//! TCP transport implementation.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use dashmap::DashMap;
use futures::stream::StreamExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{debug, error, info, warn};

use crate::codec::MessageCodec;
use crate::core::{Message, Peer, PeerId};
use crate::{Error, Result};

/// TCP transport for gossip messages.
pub struct TcpTransport {
    /// Local listening address
    local_addr: Option<SocketAddr>,

    /// Active peer connections
    peers: Arc<DashMap<SocketAddr, Peer>>,

    /// Channel for receiving messages from all peers
    message_rx: UnboundedReceiver<(SocketAddr, Message)>,

    /// Channel for sending messages (cloned to connection handlers)
    message_tx: UnboundedSender<(SocketAddr, Message)>,
}

impl TcpTransport {
    /// Create a new TCP transport.
    pub fn new() -> Self {
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        Self {
            local_addr: None,
            peers: Arc::new(DashMap::new()),
            message_rx,
            message_tx,
        }
    }

    /// Start listening on the given address.
    pub async fn listen(&mut self, addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Error::Connection { addr, source: e })?;

        let local_addr = listener.local_addr().map_err(Error::Io)?;
        self.local_addr = Some(local_addr);

        info!("TCP transport listening on {local_addr}");

        let peers = Arc::clone(&self.peers);
        let message_tx = self.message_tx.clone();

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

        info!("Connected to peer {addr}");

        Self::handle_connection(
            stream,
            addr,
            Arc::clone(&self.peers),
            self.message_tx.clone(),
        );

        Ok(())
    }

    /// Send a message to a peer.
    pub async fn send(&self, peer: SocketAddr, message: Message) -> Result<()> {
        let data = bincode::serialize(&message)?;

        if let Some(conn) = self.peers.get_mut(&peer).as_deref_mut() {
            conn.send(Bytes::from(data))
                .map_err(|err| Error::Channel(err.to_string()))?;
            Ok(())
        } else {
            Err(Error::PeerNotFound(peer))
        }
    }

    /// Receive a message from any peer.
    pub async fn recv(&mut self) -> Result<(SocketAddr, Message)> {
        self.message_rx
            .recv()
            .await
            .ok_or(Error::Channel("Channel recv error".to_string()))
    }

    /// Get local listening address.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
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
    ) {
        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();

            let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();

            let peer = Peer::new(PeerId(peer_addr), tx);
            peers.insert(peer_addr, peer);

            // Spawn writer task
            let write_task = {
                let mut sink = FramedWrite::new(writer, MessageCodec::new());
                tokio::spawn(async move {
                    while let Some(data) = rx.recv().await {
                        match bincode::deserialize::<Message>(&data) {
                            Ok(message) => {
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
                let mut stream = FramedRead::new(reader, MessageCodec::new());
                tokio::spawn(async move {
                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(message) => {
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
            info!("Disconnected from {peer_addr}");
        });
    }
}

impl Default for TcpTransport {
    fn default() -> Self {
        Self::new()
    }
}
