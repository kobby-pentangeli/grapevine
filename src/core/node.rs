//! High-level node API.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::RwLock;
use tracing::info;

use crate::{Gossip, NodeConfig, Result};

/// A Grapevine gossip node.
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
    pub async fn broadcast(&self, data: impl Into<Bytes>) -> Result<()> {
        let protocol = self.protocol.read().await;
        protocol.broadcast(data.into()).await
    }

    /// Set a handler for received application messages.
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
        protocol.peer_list()
    }

    /// Shutdown the node gracefully.
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
