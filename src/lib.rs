//! Grapevine - A modern, asynchronous peer-to-peer gossip protocol library.
//!
//! This library provides an implementation of gossip protocols for
//! distributed systems, supporting epidemic broadcast, anti-entropy, and
//! configurable transport layers.
//!
//! # Features
//!
//! - **Async/await**: Built on Tokio for efficient asynchronous I/O
//! - **Flexible transport**: TCP by default, QUIC optional
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
//!
//!     Ok(())
//! }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod codec;
pub mod config;
pub mod core;
pub mod error;
pub mod protocol;
pub mod transport;

pub use core::{Message, MessageId, Node, Payload, Peer, PeerId, PeerInfo, PeerState};

pub use codec::MessageCodec;
pub use config::{NodeConfig, NodeConfigBuilder, RateLimitConfig, RateLimiter, TransportConfig};
pub use error::{Error, Result};
pub use protocol::{AntiEntropy, AntiEntropyConfig, EpidemicConfig, Gossip};
pub use transport::Transport;
pub use transport::tcp::TcpTransport;
