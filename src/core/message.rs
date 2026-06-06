//! Message types for gossip protocol.

use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use serde::{Deserialize, Serialize};

static MESSAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Unique identifier for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId {
    /// Origin node address
    pub origin: SocketAddr,
    /// Sequence number
    pub sequence: u64,
    /// Timestamp (milliseconds since epoch)
    pub timestamp: u64,
}

impl MessageId {
    /// Create a new message ID.
    pub fn new(origin: SocketAddr) -> Self {
        let sequence = MESSAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
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

    /// Signature (if crypto feature enabled)
    #[cfg(feature = "crypto")]
    pub signature: Option<Vec<u8>>,
}

impl Message {
    /// Create a new gossip message.
    pub fn new(origin: SocketAddr, payload: Payload) -> Self {
        Self {
            id: MessageId::new(origin),
            ttl: 10, // Default TTL
            payload,
            #[cfg(feature = "crypto")]
            signature: None,
        }
    }

    /// Create with custom TTL.
    pub fn with_ttl(origin: SocketAddr, payload: Payload, ttl: u8) -> Self {
        Self {
            id: MessageId::new(origin),
            ttl,
            payload,
            #[cfg(feature = "crypto")]
            signature: None,
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

    /// Peer discovery request
    PeerDiscovery,

    /// Peer list announcement
    PeerAnnouncement {
        /// List of known peers
        peers: Vec<SocketAddr>,
    },

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

    /// Anti-entropy digest (message IDs this node knows about)
    AntiEntropyDigest {
        /// Set of known message IDs
        message_ids: Vec<MessageId>,
    },

    /// Request for specific messages by ID
    MessageRequest {
        /// Message IDs being requested
        ids: Vec<MessageId>,
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
    fn create_message_id() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let id1 = MessageId::new(addr);
        let id2 = MessageId::new(addr);
        assert_ne!(id1.sequence, id2.sequence);
        assert_eq!(id1.origin, addr);
        assert_eq!(id2.origin, addr);
    }

    #[test]
    fn decrement_ttl() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let mut msg = Message::with_ttl(addr, Payload::PeerDiscovery, 2);
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

        // Payload::PeerDiscovery
        let payload = Payload::PeerDiscovery;
        assert!(payload.is_protocol_message());

        // Payload::PeerAnnouncement
        let peers = vec![
            "127.0.0.1:8001".parse().unwrap(),
            "127.0.0.1:8002".parse().unwrap(),
        ];
        let payload = Payload::PeerAnnouncement {
            peers: peers.clone(),
        };
        assert!(payload.is_protocol_message());
        match payload {
            Payload::PeerAnnouncement { peers: p } => assert_eq!(p, peers),
            _ => panic!("Expected PeerAnnouncement payload"),
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
    fn multiple_messages_different_sequences() {
        let addr = "127.0.0.1:8000".parse().unwrap();
        let messages = (0..10)
            .map(|_| Message::new(addr, Payload::PeerDiscovery))
            .collect::<Vec<_>>();

        for i in 0..messages.len() - 1 {
            for j in i + 1..messages.len() {
                assert_ne!(messages[i].id.sequence, messages[j].id.sequence);
            }
        }
    }

    #[test]
    fn direct_message_serialization() {
        let sender = "127.0.0.1:8000".parse().unwrap();
        let recipient = "127.0.0.1:8001".parse().unwrap();
        let data = Bytes::from("test direct message");

        let message = Message::new(
            sender,
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
