//! Message types for gossip protocol.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::core::identity::{PeerId, Signature};

/// Unique identifier for a message: the originating node plus that node's
/// monotonic per-origin sequence number.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MessageId {
    /// Canonical (listening) address of the node that originated the message.
    pub origin: SocketAddr,

    /// Per-origin monotonic sequence number assigned by the originating node.
    pub sequence: u64,

    /// Creation time in milliseconds since the Unix epoch.
    ///
    /// Metadata only. Identity is `(origin, sequence)`: `timestamp` is excluded
    /// from [`PartialEq`], [`Eq`], and [`Hash`] so a node's wall clock can
    /// neither split one logical message into two keys nor collapse two distinct
    /// messages into one.
    pub timestamp: u64,
}

impl MessageId {
    /// Create an identifier for a message originated by `origin` at `sequence`.
    pub fn new(origin: SocketAddr, sequence: u64) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);

        Self {
            origin,
            sequence,
            timestamp,
        }
    }
}

impl PartialEq for MessageId {
    fn eq(&self, other: &Self) -> bool {
        self.origin == other.origin && self.sequence == other.sequence
    }
}

impl Eq for MessageId {}

impl Hash for MessageId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.origin.hash(state);
        self.sequence.hash(state);
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.origin, self.sequence, self.timestamp)
    }
}

/// The main gossip message structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message identifier
    pub id: MessageId,

    /// Time-to-live (hop count)
    pub ttl: u8,

    /// Message payload
    pub payload: Payload,

    /// The originating node's Ed25519 public key, signed over and verified
    /// against on receipt.
    pub origin_key: PeerId,

    /// Ed25519 signature over the domain-separated `(origin, sequence, payload)`.
    pub signature: Signature,
}

impl Message {
    /// Default time-to-live (hop count) assigned to a newly authored message.
    pub const DEFAULT_TTL: u8 = 10;

    /// Create an **unsigned** gossip message originated by `origin` at `sequence`.
    pub fn new(origin: SocketAddr, sequence: u64, payload: Payload) -> Self {
        Self::with_ttl(origin, sequence, payload, Self::DEFAULT_TTL)
    }

    /// Create an **unsigned** message with a custom TTL.
    pub fn with_ttl(origin: SocketAddr, sequence: u64, payload: Payload, ttl: u8) -> Self {
        Self {
            id: MessageId::new(origin, sequence),
            ttl,
            payload,
            origin_key: PeerId::UNSIGNED,
            signature: Signature::UNSIGNED,
        }
    }

    /// Decrement TTL and check if message should be propagated.
    pub fn decrement_ttl(&mut self) -> bool {
        if self.ttl > 0 {
            self.ttl -= 1;
            true
        } else {
            false
        }
    }
}

/// Message payload variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Payload {
    /// User-defined application data
    Application(Bytes),

    /// Heartbeat/keep-alive
    Heartbeat {
        /// Sender's address
        from: SocketAddr,
    },

    /// Request for peer list
    PeerListRequest,

    /// Response with peer list
    PeerListResponse {
        /// List of peers
        peers: Vec<SocketAddr>,
    },

    /// Anti-entropy digest: the sender's per-origin reconciliation summary.
    AntiEntropyDigest {
        /// For each origin, the lowest broadcast sequence the sender still
        /// needs (equivalently, the length of its contiguous prefix). The
        /// recipient pushes back every message it holds at or above this
        /// sequence, so a smaller value requests more history.
        version_vector: Vec<(SocketAddr, u64)>,
    },

    /// Pull request: the sender's per-origin reconciliation summary, asking the
    /// recipient to push every message it holds beyond these sequences.
    MessageRequest {
        /// Same shape and meaning as `AntiEntropyDigest`'s `version_vector`.
        version_vector: Vec<(SocketAddr, u64)>,
    },

    /// Response containing requested messages
    MessageResponse {
        /// The requested messages
        messages: Vec<Message>,
    },

    /// Graceful shutdown notification
    Goodbye {
        /// Reason for departure
        reason: String,
    },

    /// Direct message to a specific peer (not gossiped)
    DirectMessage {
        /// Intended recipient address
        recipient: SocketAddr,
        /// Message data
        data: Bytes,
    },
}

impl Payload {
    /// Check if this is a protocol message (vs application message).
    pub fn is_protocol_message(&self) -> bool {
        !matches!(self, Self::Application(_) | Self::DirectMessage { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_id_identity_is_origin_and_sequence_only() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let a = MessageId::new(addr, 7);
        let b = MessageId::new(addr, 7);
        let c = MessageId::new(addr, 8);

        assert_eq!(
            a, b,
            "same (origin, sequence) is one identity regardless of the clock"
        );
        assert_ne!(a, c, "a different sequence is a different message");

        let mut set = std::collections::HashSet::new();
        assert!(set.insert(a));
        assert!(!set.insert(b), "equal ids collapse to one hash-set entry");
        assert!(set.insert(c));
    }

    #[test]
    fn decrement_ttl() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let mut msg = Message::with_ttl(addr, 0, Payload::PeerListRequest, 2);
        assert!(msg.decrement_ttl());
        assert_eq!(msg.ttl, 1);
        assert!(msg.decrement_ttl());
        assert_eq!(msg.ttl, 0);
        assert!(!msg.decrement_ttl());
        assert_eq!(msg.ttl, 0);
    }

    #[test]
    fn payload_types() {
        // Payload::Application
        let data = Bytes::from("test data");
        let payload = Payload::Application(data.clone());
        assert!(!payload.is_protocol_message());
        match payload {
            Payload::Application(d) => assert_eq!(d, data),
            _ => panic!("Expected Application payload"),
        }

        // Payload::Heartbeat
        let addr = "127.0.0.1:8000".parse().unwrap();
        let payload = Payload::Heartbeat { from: addr };
        assert!(payload.is_protocol_message());
        match payload {
            Payload::Heartbeat { from } => assert_eq!(from, addr),
            _ => panic!("Expected Heartbeat payload"),
        }

        // Payload::PeerListRequest
        let payload = Payload::PeerListRequest;
        assert!(payload.is_protocol_message());

        // Payload::PeerListResponse
        let peers = vec!["127.0.0.1:8001".parse().unwrap()];
        let payload = Payload::PeerListResponse {
            peers: peers.clone(),
        };
        assert!(payload.is_protocol_message());
        match payload {
            Payload::PeerListResponse { peers: p } => assert_eq!(p, peers),
            _ => panic!("Expected PeerListResponse payload"),
        }

        // Payload::DirectMessage
        let recipient = "127.0.0.1:8001".parse().unwrap();
        let data = Bytes::from("private message");
        let payload = Payload::DirectMessage {
            recipient,
            data: data.clone(),
        };
        assert!(!payload.is_protocol_message());
        match payload {
            Payload::DirectMessage {
                recipient: r,
                data: d,
            } => {
                assert_eq!(r, recipient);
                assert_eq!(d, data);
            }
            _ => panic!("Expected DirectMessage payload"),
        }

        // Payload::Goodbye
        let reason = "Normal shutdown".to_string();
        let payload = Payload::Goodbye {
            reason: reason.clone(),
        };
        assert!(payload.is_protocol_message());
        match payload {
            Payload::Goodbye { reason: r } => assert_eq!(r, reason),
            _ => panic!("Expected Goodbye payload"),
        }
    }

    #[test]
    fn message_carries_its_explicit_sequence() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let messages = (0u64..10)
            .map(|seq| Message::new(addr, seq, Payload::PeerListRequest))
            .collect::<Vec<_>>();

        for (index, message) in messages.iter().enumerate() {
            assert_eq!(message.id.sequence, u64::try_from(index).unwrap());
        }
    }

    #[test]
    fn direct_message_serialization() {
        let sender = "127.0.0.1:8000".parse().unwrap();
        let recipient = "127.0.0.1:8001".parse().unwrap();
        let data = Bytes::from("test direct message");

        let message = Message::new(
            sender,
            0,
            Payload::DirectMessage {
                recipient,
                data: data.clone(),
            },
        );

        let serialized =
            bincode::serde::encode_to_vec(&message, bincode::config::standard()).unwrap();
        let (deserialized, _): (Message, _) =
            bincode::serde::decode_from_slice(&serialized, bincode::config::standard()).unwrap();

        assert_eq!(message.id, deserialized.id);
        assert_eq!(message.ttl, deserialized.ttl);

        match deserialized.payload {
            Payload::DirectMessage {
                recipient: r,
                data: d,
            } => {
                assert_eq!(r, recipient);
                assert_eq!(d, data);
            }
            _ => panic!("Expected DirectMessage payload after deserialization"),
        }
    }
}
