//! High-level node API.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::RwLock;
use tracing::info;

use crate::{Gossip, NodeConfig, Result};

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
///         println!("Got message from {}: {:?}", origin, data);
///     }).await;
///
///     node.start().await?;
///     node.broadcast(Bytes::from("Hello!")).await?;
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
    protocol: Arc<RwLock<Gossip>>,
}

impl Node {
    /// Create a new node with the given configuration.
    pub async fn new(config: NodeConfig) -> Result<Self> {
        let protocol = Gossip::new(config.clone());

        Ok(Self {
            config,
            protocol: Arc::new(RwLock::new(protocol)),
        })
    }

    /// Start the node.
    pub async fn start(&self) -> Result<()> {
        let mut protocol = self.protocol.write().await;
        protocol.start().await?;
        info!("Node started successfully");
        Ok(())
    }

    /// Broadcast a message to the network.
    ///
    /// Messages are propagated using epidemic broadcast with configurable
    /// forward probability and anti-entropy for reliability.
    pub async fn broadcast(&self, data: impl Into<Bytes>) -> Result<()> {
        let protocol = self.protocol.read().await;
        protocol.broadcast(data.into()).await
    }

    /// Set a handler for received application messages.
    ///
    /// The handler is called for each received application message with the
    /// originating peer address and message payload.
    pub async fn on_message<F>(&self, handler: F)
    where
        F: Fn(SocketAddr, Bytes) + Send + Sync + 'static,
    {
        let mut protocol = self.protocol.write().await;
        protocol.set_message_handler(handler);
    }

    /// Get the node's local address.
    pub async fn local_addr(&self) -> Option<SocketAddr> {
        let protocol = self.protocol.read().await;
        protocol.local_addr().await
    }

    /// Get connected peer addresses.
    pub async fn peers(&self) -> Vec<SocketAddr> {
        let protocol = self.protocol.read().await;
        protocol.peer_list().await
    }

    /// Shutdown the node gracefully.
    ///
    /// This sends goodbye messages to all connected peers, stops all background
    /// tasks, and cleans up resources. The shutdown process typically completes
    /// within 500ms.
    pub async fn shutdown(&self) -> Result<()> {
        let protocol = self.protocol.read().await;
        protocol.shutdown().await?;
        info!("Node shut down");
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
