//! Grapevine: a modern, asynchronous peer-to-peer gossip protocol library.
//!
//! This library provides an implementation of gossip protocols for
//! distributed systems, supporting epidemic broadcast, anti-entropy, and
//! configurable transport layers.
//!
//! # Features
//!
//! - **Async/await**: Built on Tokio for efficient asynchronous I/O
//! - **Authenticated messages**: Every message is Ed25519-signed by its origin
//!   and verified on receipt (see [`core::identity`] for the threat model)
//! - **Flexible transport**: TCP by default, QUIC [scheduled for v1.1+]
//! - **Configurable**: Extensive configuration options
//!
//! # Example
//!
//! ```rust,no_run
//! use grapevine::{NodeConfig, Node};
//! use bytes::Bytes;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = NodeConfig::default();
//!     let node = Node::new(config).await?;
//!
//!     node.on_message(|origin, data| {
//!         println!("Received from {origin}: {data:?}");
//!     }).await;
//!
//!     node.start().await?;
//!     node.broadcast(Bytes::from("Hello, gossip!")).await?;
//!     node.shutdown().await?;
//!
//!     Ok(())
//! }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod core;
pub mod error;
pub mod node;
pub mod protocol;
pub mod transport;

pub use core::{
    Identity, Message, MessageCodec, MessageId, Payload, Peer, PeerId, PeerInfo, PeerState,
    RateLimitConfig, RateLimiter, Signature, authenticate, verify_message,
};

pub use error::Error;
pub use node::{Node, NodeConfig, NodeConfigBuilder};
pub use protocol::{AntiEntropy, AntiEntropyConfig, EpidemicConfig, Gossip, MessageEntry};
pub use transport::{Tcp, TransportConfig};

/// Result type alias for all operations.
pub type Result<T> = std::result::Result<T, Error>;
