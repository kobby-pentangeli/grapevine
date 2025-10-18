//! Node configuration structures.

use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Configuration for a Grapevine node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Address to bind the node to
    pub bind_addr: SocketAddr,

    /// Initial peers to connect to
    pub bootstrap_peers: Vec<SocketAddr>,

    /// Gossip interval (how often to gossip)
    pub gossip_interval: Duration,

    /// Fan-out factor (how many peers to gossip to)
    pub fanout: usize,

    /// Maximum message size in bytes
    pub max_message_size: usize,

    /// Peer timeout duration
    pub peer_timeout: Duration,

    /// Maximum number of peers to maintain
    pub max_peers: usize,

    /// Connection timeout
    pub connection_timeout: Duration,

    /// Enable message signing (requires 'crypto' feature)
    #[cfg(feature = "crypto")]
    pub enable_signing: bool,

    /// Transport protocol
    pub transport: TransportConfig,
}

/// Transport protocol configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportConfig {
    /// TCP transport
    Tcp,
    /// QUIC transport (requires 'quic' feature)
    #[cfg(feature = "quic")]
    Quic {
        /// Path to certificate file
        cert_path: Option<String>,
        /// Path to private key file
        key_path: Option<String>,
    },
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:0".parse().expect("valid address"),
            bootstrap_peers: Vec::new(),
            gossip_interval: Duration::from_secs(5),
            fanout: 3,
            max_message_size: 1024 * 1024, // 1 MB
            peer_timeout: Duration::from_secs(30),
            max_peers: 50,
            connection_timeout: Duration::from_secs(10),
            #[cfg(feature = "crypto")]
            enable_signing: false,
            transport: TransportConfig::Tcp,
        }
    }
}

/// Builder for NodeConfig.
#[derive(Debug, Default)]
pub struct NodeConfigBuilder {
    config: NodeConfig,
}

impl NodeConfigBuilder {
    /// Create a new builder with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set bind address.
    pub fn bind_addr(mut self, addr: SocketAddr) -> Self {
        self.config.bind_addr = addr;
        self
    }

    /// Add a bootstrap peer.
    pub fn add_bootstrap_peer(mut self, peer: SocketAddr) -> Self {
        self.config.bootstrap_peers.push(peer);
        self
    }

    /// Set bootstrap peers.
    pub fn bootstrap_peers(mut self, peers: Vec<SocketAddr>) -> Self {
        self.config.bootstrap_peers = peers;
        self
    }

    /// Set gossip interval.
    pub fn gossip_interval(mut self, interval: Duration) -> Self {
        self.config.gossip_interval = interval;
        self
    }

    /// Set fan-out factor.
    pub fn fanout(mut self, fanout: usize) -> Self {
        self.config.fanout = fanout;
        self
    }

    /// Set maximum message size.
    pub fn max_message_size(mut self, size: usize) -> Self {
        self.config.max_message_size = size;
        self
    }

    /// Set peer timeout.
    pub fn peer_timeout(mut self, timeout: Duration) -> Self {
        self.config.peer_timeout = timeout;
        self
    }

    /// Set maximum peers.
    pub fn max_peers(mut self, max: usize) -> Self {
        self.config.max_peers = max;
        self
    }

    /// Set connection timeout.
    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.config.connection_timeout = timeout;
        self
    }

    /// Enable message signing.
    #[cfg(feature = "crypto")]
    pub fn enable_signing(mut self, enable: bool) -> Self {
        self.config.enable_signing = enable;
        self
    }

    /// Set transport configuration.
    pub fn transport(mut self, transport: TransportConfig) -> Self {
        self.config.transport = transport;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> Result<NodeConfig> {
        self.validate()?;
        Ok(self.config)
    }

    fn validate(&self) -> Result<()> {
        if self.config.fanout == 0 {
            return Err(Error::Config("fanout must be > 0".into()));
        }
        if self.config.max_peers == 0 {
            return Err(Error::Config("max_peers must be > 0".into()));
        }
        if self.config.max_message_size == 0 {
            return Err(Error::Config("max_message_size must be > 0".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = NodeConfig::default();
        assert_eq!(config.fanout, 3);
        assert_eq!(config.max_peers, 50);
    }

    #[test]
    fn config_builder() {
        let config = NodeConfigBuilder::new()
            .fanout(5)
            .max_peers(100)
            .build()
            .unwrap();
        assert_eq!(config.fanout, 5);
        assert_eq!(config.max_peers, 100);
    }

    #[test]
    fn validate_fanout() {
        let result = NodeConfigBuilder::new().fanout(0).build();
        assert!(result.is_err());
    }
}
