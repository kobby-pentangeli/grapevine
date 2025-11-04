//! Node configuration structures.

use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::RateLimitConfig;
use crate::{AntiEntropyConfig, EpidemicConfig, Error, Result};

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

    /// Message deduplication TTL (how long to remember seen messages)
    pub message_dedup_ttl: Duration,

    /// Anti-entropy protocol configuration
    pub anti_entropy: AntiEntropyConfig,

    /// Epidemic broadcast configuration
    pub epidemic: EpidemicConfig,

    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,

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
            message_dedup_ttl: Duration::from_secs(300), // 5 minutes
            anti_entropy: AntiEntropyConfig::default(),
            epidemic: EpidemicConfig::default(),
            rate_limit: RateLimitConfig::default(),
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

    /// Set message deduplication TTL.
    pub fn message_dedup_ttl(mut self, ttl: Duration) -> Self {
        self.config.message_dedup_ttl = ttl;
        self
    }

    /// Set anti-entropy configuration.
    pub fn anti_entropy(mut self, config: AntiEntropyConfig) -> Self {
        self.config.anti_entropy = config;
        self
    }

    /// Set epidemic broadcast configuration.
    pub fn epidemic(mut self, config: EpidemicConfig) -> Self {
        self.config.epidemic = config;
        self
    }

    /// Set rate limiting configuration.
    pub fn rate_limit(mut self, config: RateLimitConfig) -> Self {
        self.config.rate_limit = config;
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
        if self.config.fanout > self.config.max_peers {
            return Err(Error::Config("fanout cannot exceed max_peers".into()));
        }
        if self.config.gossip_interval < Duration::from_secs(1) {
            return Err(Error::Config("gossip_interval must be >= 1 second".into()));
        }
        if self.config.gossip_interval > Duration::from_secs(3600) {
            return Err(Error::Config("gossip_interval must be <= 1 hour".into()));
        }
        if self.config.peer_timeout < Duration::from_secs(5) {
            return Err(Error::Config("peer_timeout must be >= 5 seconds".into()));
        }
        if self.config.connection_timeout < Duration::from_secs(1) {
            return Err(Error::Config(
                "connection_timeout must be >= 1 second".into(),
            ));
        }
        if self.config.rate_limit.enabled {
            if self.config.rate_limit.capacity == 0 {
                return Err(Error::Config(
                    "rate_limit capacity must be > 0 when enabled".into(),
                ));
            }
            if self.config.rate_limit.refill_rate == 0 {
                return Err(Error::Config(
                    "rate_limit refill_rate must be > 0 when enabled".into(),
                ));
            }
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
        assert_eq!(config.max_message_size, 1024 * 1024);
        assert_eq!(config.gossip_interval, Duration::from_secs(5));
        assert_eq!(config.peer_timeout, Duration::from_secs(30));
        assert_eq!(config.connection_timeout, Duration::from_secs(10));
        assert!(config.bootstrap_peers.is_empty());
        assert!(matches!(config.transport, TransportConfig::Tcp));
    }

    #[test]
    fn config_builder_basic() {
        let config = NodeConfigBuilder::new()
            .fanout(5)
            .max_peers(100)
            .build()
            .unwrap();
        assert_eq!(config.fanout, 5);
        assert_eq!(config.max_peers, 100);
    }

    #[test]
    fn config_builder_all_fields() {
        let bind_addr = "192.168.1.1:9000".parse().unwrap();
        let peer1 = "192.168.1.2:9000".parse().unwrap();
        let peer2 = "192.168.1.3:9000".parse().unwrap();

        let config = NodeConfigBuilder::new()
            .bind_addr(bind_addr)
            .add_bootstrap_peer(peer1)
            .add_bootstrap_peer(peer2)
            .fanout(7)
            .max_peers(200)
            .max_message_size(2048)
            .gossip_interval(Duration::from_secs(10))
            .peer_timeout(Duration::from_secs(60))
            .connection_timeout(Duration::from_secs(20))
            .build()
            .unwrap();

        assert_eq!(config.bind_addr, bind_addr);
        assert_eq!(config.bootstrap_peers.len(), 2);
        assert_eq!(config.bootstrap_peers[0], peer1);
        assert_eq!(config.bootstrap_peers[1], peer2);
        assert_eq!(config.fanout, 7);
        assert_eq!(config.max_peers, 200);
        assert_eq!(config.max_message_size, 2048);
        assert_eq!(config.gossip_interval, Duration::from_secs(10));
        assert_eq!(config.peer_timeout, Duration::from_secs(60));
        assert_eq!(config.connection_timeout, Duration::from_secs(20));
    }

    #[test]
    fn config_builder_bootstrap_peers() {
        let peer1 = "192.168.1.2:9000".parse().unwrap();
        let peer2 = "192.168.1.3:9000".parse().unwrap();
        let peers = vec![peer1, peer2];

        let config = NodeConfigBuilder::new()
            .bootstrap_peers(peers.clone())
            .build()
            .unwrap();

        assert_eq!(config.bootstrap_peers, peers);
    }

    #[test]
    fn validate_fanout_zero() {
        let result = NodeConfigBuilder::new().fanout(0).build();
        assert!(result.is_err());
        match result {
            Err(Error::Config(msg)) => assert!(msg.contains("fanout")),
            _ => panic!("Expected Config error"),
        }
    }

    #[test]
    fn validate_max_peers_zero() {
        let result = NodeConfigBuilder::new().max_peers(0).build();
        assert!(result.is_err());
        match result {
            Err(Error::Config(msg)) => assert!(msg.contains("max_peers")),
            _ => panic!("Expected Config error"),
        }
    }

    #[test]
    fn validate_max_message_size_zero() {
        let result = NodeConfigBuilder::new().max_message_size(0).build();
        assert!(result.is_err());
        match result {
            Err(Error::Config(msg)) => assert!(msg.contains("max_message_size")),
            _ => panic!("Expected Config error"),
        }
    }

    #[test]
    fn validate_all_valid() {
        let config = NodeConfigBuilder::new()
            .fanout(1)
            .max_peers(1)
            .max_message_size(1)
            .build();
        assert!(config.is_ok());
    }

    #[test]
    fn config_serialization() {
        let config = NodeConfig::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: NodeConfig = serde_json::from_str(&serialized).unwrap();

        assert_eq!(config.fanout, deserialized.fanout);
        assert_eq!(config.max_peers, deserialized.max_peers);
        assert_eq!(config.max_message_size, deserialized.max_message_size);
    }

    #[test]
    fn transport_config_tcp() {
        let config = NodeConfigBuilder::new()
            .transport(TransportConfig::Tcp)
            .build()
            .unwrap();
        assert!(matches!(config.transport, TransportConfig::Tcp));
    }
}
