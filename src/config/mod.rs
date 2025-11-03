//! Configuration types and builders for Grapevine nodes.

pub mod node_config;

pub use node_config::{NodeConfig, NodeConfigBuilder, TransportConfig};

/// Serde helper for Duration serialization.
pub mod serde_duration {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize a Duration as seconds.
    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_secs())
    }

    /// Deserialize a Duration from seconds.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}
