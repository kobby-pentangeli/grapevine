//! Peer management types.

use std::fmt;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{Error, Result};

/// Unique identifier for a peer (currently just their socket address).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub SocketAddr);

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<SocketAddr> for PeerId {
    fn from(addr: SocketAddr) -> Self {
        Self(addr)
    }
}

/// State of a peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// Connection is being established
    Connecting,
    /// Connection is established and healthy
    Connected,
    /// Peer is connected but unresponsive
    Stale,
    /// Peer is disconnected
    Disconnected,
}

/// Information about a peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer identifier
    pub id: PeerId,

    /// Current state
    pub state: PeerState,

    /// Last time we received a message from this peer
    pub last_seen: Instant,

    /// When the connection was established
    pub connected_at: Instant,

    /// Number of messages received from this peer
    pub messages_received: u64,

    /// Number of messages sent to this peer
    pub messages_sent: u64,
}

impl PeerInfo {
    /// Create new peer info.
    pub fn new(id: PeerId) -> Self {
        let now = Instant::now();
        Self {
            id,
            state: PeerState::Connecting,
            last_seen: now,
            connected_at: now,
            messages_received: 0,
            messages_sent: 0,
        }
    }

    /// Mark peer as connected.
    pub fn mark_connected(&mut self) {
        self.state = PeerState::Connected;
    }

    /// Update last seen timestamp.
    pub fn update_last_seen(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Check if peer is stale (hasn't been seen recently).
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_seen.elapsed() > timeout
    }

    /// Increment received message counter.
    pub fn increment_received(&mut self) {
        self.messages_received = self.messages_received.saturating_add(1);
        self.update_last_seen();
    }

    /// Increment sent message counter.
    pub fn increment_sent(&mut self) {
        self.messages_sent = self.messages_sent.saturating_add(1);
    }
}

/// Peer handle for network operations.
#[derive(Debug)]
pub struct Peer {
    /// Peer information
    pub info: PeerInfo,

    /// Channel for sending messages to this peer
    sender: UnboundedSender<Bytes>,
}

impl Peer {
    /// Create a new peer.
    pub fn new(id: PeerId, sender: UnboundedSender<Bytes>) -> Self {
        Self {
            info: PeerInfo::new(id),
            sender,
        }
    }

    /// Send data to this peer.
    pub fn send(&mut self, data: Bytes) -> Result<()> {
        self.sender
            .send(data)
            .map_err(|err| Error::Channel(err.to_string()))?;
        self.info.increment_sent();
        Ok(())
    }

    /// Get peer ID.
    pub fn id(&self) -> PeerId {
        self.info.id
    }

    /// Get peer state.
    pub fn state(&self) -> PeerState {
        self.info.state
    }
}
