//! Epidemic broadcast protocol.
//!
//! Implements probabilistic message dissemination.

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for epidemic broadcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpidemicConfig {
    /// Probability of forwarding a message (0.0 - 1.0)
    pub forward_probability: f64,

    /// Maximum number of times to forward
    pub max_forwards: u32,
}

impl EpidemicConfig {
    /// Determine if message should be forwarded.
    pub fn should_forward(&self) -> bool {
        let mut rng = rand::rng();
        rng.random::<f64>() < self.forward_probability
    }
}

impl Default for EpidemicConfig {
    fn default() -> Self {
        Self {
            forward_probability: 0.7,
            max_forwards: 5,
        }
    }
}
