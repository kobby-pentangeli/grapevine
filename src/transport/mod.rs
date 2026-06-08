//! Network transport implementations.

pub mod tcp;

use serde::{Deserialize, Serialize};
pub use tcp::Tcp;

/// Transport protocol configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportConfig {
    /// TCP transport
    Tcp,
}
