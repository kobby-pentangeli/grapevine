//! High-level node API.

pub mod node_config;

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
pub use node_config::{NodeConfig, NodeConfigBuilder};
use tracing::trace;

use crate::{Gossip, Result};

/// A Grapevine gossip node.
///
/// `Node` is the main entry point for using Grapevine. It manages connections
/// to peers, handles message routing, and provides a high-level API for
/// broadcasting messages.
///
/// # Examples
///
/// ## Basic usage
///
/// ```rust,no_run
/// use grapevine::{Node, NodeConfig};
/// use bytes::Bytes;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = NodeConfig::default();
///     let node = Node::new(config).await?;
///
///     node.on_message(|origin, data| {
///         println!("Got message from {origin}: {data:?}");
///     }).await;
///
///     node.start().await?;
///     node.broadcast(Bytes::from("Hello!")).await?;
///     node.shutdown().await?;
///
///     Ok(())
/// }
/// ```
///
/// ## With custom configuration
///
/// ```rust,no_run
/// use grapevine::{Node, NodeConfigBuilder};
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = NodeConfigBuilder::new()
///         .gossip_interval(Duration::from_secs(3))
///         .fanout(5)
///         .build()?;
///
///     let node = Node::new(config).await?;
///     node.start().await?;
///
///     Ok(())
/// }
/// ```
pub struct Node {
    /// Node configuration
    pub config: NodeConfig,

    /// Gossip protocol engine
    protocol: Arc<Gossip>,
}

impl Node {
    /// Create a new node with the given configuration.
    pub async fn new(config: NodeConfig) -> Result<Self> {
        let protocol = Gossip::new(config.clone())?;

        Ok(Self {
            config,
            protocol: Arc::new(protocol),
        })
    }

    /// Start the node.
    pub async fn start(&self) -> Result<()> {
        self.protocol.start().await?;
        trace!("Node started");
        Ok(())
    }

    /// Broadcast a message to the network.
    ///
    /// Messages are propagated using epidemic broadcast with configurable
    /// forward probability and anti-entropy for reliability.
    pub async fn broadcast(&self, data: impl Into<Bytes>) -> Result<()> {
        self.protocol.broadcast(data.into()).await
    }

    /// Send a direct message to a specific peer.
    ///
    /// Unlike broadcast, direct messages are only delivered to the specified
    /// recipient and are not propagated through the gossip network.
    ///
    /// # Arguments
    ///
    /// * `peer` - The recipient's socket address
    /// * `data` - The message payload
    ///
    /// # Errors
    ///
    /// Returns an error if the peer is not connected or if sending fails.
    pub async fn send_to_peer(&self, peer: SocketAddr, data: impl Into<Bytes>) -> Result<()> {
        self.protocol.send_to_peer(peer, data.into()).await
    }

    /// Set a handler for received application messages.
    ///
    /// The handler is called for each received application message with the
    /// originating peer address and message payload.
    pub async fn on_message<F>(&self, handler: F)
    where
        F: Fn(SocketAddr, Bytes) + Send + Sync + 'static,
    {
        self.protocol.set_message_handler(handler);
    }

    /// Get the node's local address.
    pub async fn local_addr(&self) -> Option<SocketAddr> {
        self.protocol.local_addr().await
    }

    /// Get connected peer addresses.
    pub async fn peers(&self) -> Vec<SocketAddr> {
        self.protocol.peer_list().await
    }

    /// Shutdown the node gracefully.
    ///
    /// This sends goodbye messages to all connected peers, stops all background
    /// tasks, and cleans up resources. The shutdown process typically completes
    /// within 500ms.
    pub async fn shutdown(&self) -> Result<()> {
        self.protocol.shutdown().await?;
        trace!("Node shut down");
        Ok(())
    }
}

impl Clone for Node {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            protocol: Arc::clone(&self.protocol),
        }
    }
}
