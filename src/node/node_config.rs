//! Implements `Node` configuration.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::message_codec::MAX_FRAME_SIZE;
use crate::{AntiEntropyConfig, EpidemicConfig, Error, RateLimitConfig, Result, TransportConfig};

const DEFAULT_BIND_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);

/// Configuration for a Grapevine node.
///
/// A `NodeConfig` cannot be constructed in an invalid state: every path that
/// produces one---[`NodeConfigBuilder::build`] and `serde` deserialization (via
/// [`NodeConfig::validate`])---rejects out-of-range values rather than silently
/// accepting them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "NodeConfigUnchecked")]
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

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            bind_addr: DEFAULT_BIND_ADDR,
            bootstrap_peers: Vec::new(),
            gossip_interval: Duration::from_secs(5),
            fanout: 3,
            max_message_size: MAX_FRAME_SIZE,
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

impl NodeConfig {
    /// Validate every configuration invariant.
    ///
    /// This is the single source of truth for what a well-formed `NodeConfig`
    /// is. It runs both from [`NodeConfigBuilder::build`] and from `serde`
    /// deserialization, so an invalid configuration cannot reach the rest of
    /// the system through any path.
    ///
    /// # Errors
    /// Returns [`Error::Config`] describing the first invariant that fails.
    pub fn validate(&self) -> Result<()> {
        if self.fanout == 0 {
            return Err(Error::Config("fanout must be > 0".into()));
        }
        if self.max_peers == 0 {
            return Err(Error::Config("max_peers must be > 0".into()));
        }
        if self.max_message_size == 0 {
            return Err(Error::Config("max_message_size must be > 0".into()));
        }
        if self.fanout > self.max_peers {
            return Err(Error::Config("fanout cannot exceed max_peers".into()));
        }
        if self.gossip_interval < Duration::from_secs(1) {
            return Err(Error::Config("gossip_interval must be >= 1 second".into()));
        }
        if self.gossip_interval > Duration::from_secs(3600) {
            return Err(Error::Config("gossip_interval must be <= 1 hour".into()));
        }
        if self.peer_timeout < Duration::from_secs(5) {
            return Err(Error::Config("peer_timeout must be >= 5 seconds".into()));
        }
        if self.connection_timeout < Duration::from_secs(1) {
            return Err(Error::Config(
                "connection_timeout must be >= 1 second".into(),
            ));
        }
        if self.rate_limit.enabled {
            self.rate_limit.validate().map_err(Error::Config)?;
        }
        Ok(())
    }
}

/// Unvalidated wire representation of [`NodeConfig`].
///
/// `serde` deserializes into this first; the [`TryFrom`] conversion then
/// runs [`NodeConfig::validate`], which is what makes an invalid `NodeConfig`
/// impossible to deserialize.
#[derive(Deserialize)]
struct NodeConfigUnchecked {
    bind_addr: SocketAddr,
    bootstrap_peers: Vec<SocketAddr>,
    gossip_interval: Duration,
    fanout: usize,
    max_message_size: usize,
    peer_timeout: Duration,
    max_peers: usize,
    connection_timeout: Duration,
    message_dedup_ttl: Duration,
    anti_entropy: AntiEntropyConfig,
    epidemic: EpidemicConfig,
    rate_limit: RateLimitConfig,
    #[cfg(feature = "crypto")]
    enable_signing: bool,
    transport: TransportConfig,
}

impl TryFrom<NodeConfigUnchecked> for NodeConfig {
    type Error = Error;

    fn try_from(raw: NodeConfigUnchecked) -> Result<Self> {
        let config = Self {
            bind_addr: raw.bind_addr,
            bootstrap_peers: raw.bootstrap_peers,
            gossip_interval: raw.gossip_interval,
            fanout: raw.fanout,
            max_message_size: raw.max_message_size,
            peer_timeout: raw.peer_timeout,
            max_peers: raw.max_peers,
            connection_timeout: raw.connection_timeout,
            message_dedup_ttl: raw.message_dedup_ttl,
            anti_entropy: raw.anti_entropy,
            epidemic: raw.epidemic,
            rate_limit: raw.rate_limit,
            #[cfg(feature = "crypto")]
            enable_signing: raw.enable_signing,
            transport: raw.transport,
        };
        config.validate()?;
        Ok(config)
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
    ///
    /// # Errors
    /// Returns [`Error::Config`] if any invariant in [`NodeConfig::validate`]
    /// fails.
    pub fn build(self) -> Result<NodeConfig> {
        self.config.validate()?;
        Ok(self.config)
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
        assert_eq!(config.max_message_size, MAX_FRAME_SIZE);
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

    #[test]
    fn invalid_config_rejected_on_deserialize() {
        let valid = serde_json::to_value(NodeConfig::default()).unwrap();
        assert!(serde_json::from_value::<NodeConfig>(valid.clone()).is_ok());

        let mut bad_fanout = valid.clone();
        bad_fanout["fanout"] = serde_json::json!(0);
        assert!(serde_json::from_value::<NodeConfig>(bad_fanout).is_err());

        let mut bad_rate_limit = valid;
        bad_rate_limit["rate_limit"]["capacity"] = serde_json::json!(0);
        assert!(serde_json::from_value::<NodeConfig>(bad_rate_limit).is_err());
    }
}
