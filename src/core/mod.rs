//! Core types for Grapevine protocol.

pub mod identity;
pub mod message;
pub mod message_codec;
pub mod peer;
pub mod rate_limiter;

pub use identity::{Identity, PeerId, Signature, authenticate, verify_message};
pub use message::{Message, MessageId, Payload};
pub use message_codec::MessageCodec;
pub use peer::{Peer, PeerInfo, PeerState};
pub use rate_limiter::{RateLimitConfig, RateLimiter};
