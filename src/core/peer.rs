//! Peer management types.

use std::fmt;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{Error, Result};

/// Age bonus divisor for health score calculation (seconds).
const AGE_BONUS_DIVISOR: f64 = 300.0;

/// Maximum age bonus for health score.
const MAX_AGE_BONUS: f64 = 0.2;

/// Penalty per consecutive failure for health score.
const CONSECUTIVE_FAILURE_PENALTY: f64 = 0.1;

/// Maximum consecutive failure penalty for health score.
const MAX_CONSECUTIVE_PENALTY: f64 = 0.5;

/// Maximum consecutive failures before peer disconnection.
const MAX_CONSECUTIVE_FAILURES: u64 = 5;

/// Unique identifier for a peer (currently just its socket address).
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

    /// Number of failed send attempts
    pub message_failures: u64,

    /// Consecutive failures (reset on success)
    pub consecutive_failures: u64,
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
            message_failures: 0,
            consecutive_failures: 0,
        }
    }

    /// Mark peer as connected.
    pub fn mark_connected(&mut self) {
        self.state = PeerState::Connected;
    }

    /// Mark peer as stale.
    pub fn mark_stale(&mut self) {
        self.state = PeerState::Stale;
    }

    /// Mark peer as disconnected.
    pub fn mark_disconnected(&mut self) {
        self.state = PeerState::Disconnected;
    }

    /// Update last seen timestamp.
    pub fn update_last_seen(&mut self) {
        self.last_seen = Instant::now();
        if matches!(self.state, PeerState::Connecting | PeerState::Stale) {
            self.state = PeerState::Connected;
        }
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
        self.consecutive_failures = 0;
    }

    /// Record a failure in sending a message.
    pub fn record_failure(&mut self) {
        self.message_failures = self.message_failures.saturating_add(1);
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
    }

    /// Calculate health score (0.0 = poor, 1.0 = excellent).
    pub fn health_score(&self) -> f64 {
        let total_attempts = self.messages_sent.saturating_add(self.message_failures);
        if total_attempts == 0 {
            return 1.0;
        }

        let sent = f64::from(u32::try_from(self.messages_sent).unwrap_or(u32::MAX));
        let total = f64::from(u32::try_from(total_attempts).unwrap_or(u32::MAX));
        let success_rate = sent / total;

        let age_seconds =
            f64::from(u32::try_from(self.connected_at.elapsed().as_secs()).unwrap_or(u32::MAX));
        let age_bonus = (age_seconds / AGE_BONUS_DIVISOR).min(MAX_AGE_BONUS);

        let consecutive_penalty =
            (f64::from(u32::try_from(self.consecutive_failures).unwrap_or(u32::MAX))
                * CONSECUTIVE_FAILURE_PENALTY)
                .min(MAX_CONSECUTIVE_PENALTY);

        (success_rate + age_bonus - consecutive_penalty).clamp(0.0, 1.0)
    }

    /// Check if peer should be disconnected due to failures.
    pub fn should_disconnect(&self) -> bool {
        self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES
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
        match self.sender.send(data) {
            Ok(()) => {
                self.info.increment_sent();
                Ok(())
            }
            Err(err) => {
                self.info.record_failure();
                Err(Error::Channel(err.to_string()))
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_last_seen_drives_state_machine() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let mut info = PeerInfo::new(PeerId(addr));
        assert_eq!(info.state, PeerState::Connecting);

        info.update_last_seen();
        assert_eq!(info.state, PeerState::Connected);

        info.mark_stale();
        assert_eq!(info.state, PeerState::Stale);
        info.update_last_seen();
        assert_eq!(info.state, PeerState::Connected);

        info.mark_disconnected();
        info.update_last_seen();
        assert_eq!(info.state, PeerState::Disconnected);
    }

    #[test]
    fn peer_info_saturating_counters() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let mut info = PeerInfo::new(PeerId(addr));

        info.messages_received = u64::MAX;
        info.increment_received();
        assert_eq!(info.messages_received, u64::MAX);

        info.messages_sent = u64::MAX;
        info.increment_sent();
        assert_eq!(info.messages_sent, u64::MAX);
    }

    #[test]
    fn peer_send_success() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let peer_id = PeerId(addr);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut peer = Peer::new(peer_id, tx);
        let data = Bytes::from("test data");

        assert_eq!(peer.info.messages_sent, 0);
        let result = peer.send(data.clone());
        assert!(result.is_ok());
        assert_eq!(peer.info.messages_sent, 1);

        // Verify data was sent
        let received = rx.try_recv();
        assert!(received.is_ok());
        assert_eq!(received.unwrap(), data);
    }

    #[test]
    fn peer_send_failure_channel_closed() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let peer_id = PeerId(addr);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Drop receiver to close channel
        drop(rx);

        let mut peer = Peer::new(peer_id, tx);
        let data = Bytes::from("test data");

        let result = peer.send(data);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Channel(_)));
    }

    #[test]
    fn peer_multiple_sends() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let peer_id = PeerId(addr);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut peer = Peer::new(peer_id, tx);

        for i in 0..5 {
            let data = Bytes::from(format!("message {i}"));
            assert!(peer.send(data.clone()).is_ok());
            assert_eq!(peer.info.messages_sent, i + 1);

            let received = rx.try_recv().unwrap();
            assert_eq!(received, Bytes::from(format!("message {i}")));
        }
    }

    #[test]
    fn peer_id_serialization() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let peer_id = PeerId(addr);

        let serialized = serde_json::to_string(&peer_id).unwrap();
        let deserialized: PeerId = serde_json::from_str(&serialized).unwrap();

        assert_eq!(peer_id, deserialized);
    }
}
