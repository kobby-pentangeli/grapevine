//! Network transport implementations.

pub mod tcp;

use std::net::SocketAddr;

use async_trait::async_trait;
use bytes::Bytes;

use crate::Result;

/// Trait for network transports.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Listen on an address.
    async fn listen(&mut self, addr: SocketAddr) -> Result<()>;

    /// Connect to a peer.
    async fn connect(mut self, addr: SocketAddr) -> Result<()>;

    /// Send data to a peer.
    async fn send(&self, peer: SocketAddr, data: Bytes) -> Result<()>;

    /// Receive data from any peer.
    async fn recv(&mut self) -> Result<(SocketAddr, Bytes)>;
}
