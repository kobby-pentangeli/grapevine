//! Epidemic broadcast protocol.
//!
//! Probabilistic rumor mongering in the "blind" variant of Demers et al. 1987
//! (§1.3): a node that learns a new rumor forwards it once, with a fixed
//! probability, to a random fanout of its peers.

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for epidemic broadcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpidemicConfig {
    /// Probability in `[0.0, 1.0]` that a node forwards a rumor when it first
    /// learns it. `0.0` disables forwarding entirely; `1.0` always forwards.
    pub forward_probability: f64,
}

impl Default for EpidemicConfig {
    fn default() -> Self {
        Self {
            forward_probability: 0.7,
        }
    }
}

impl EpidemicConfig {
    /// Roll the per-rumor infection gate: whether to forward a newly learned
    /// rumor to a fresh fanout of peers.
    pub fn should_forward(&self) -> bool {
        let mut rng = rand::rng();
        rng.random::<f64>() < self.forward_probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_probability_bounds_are_deterministic() {
        let never = EpidemicConfig {
            forward_probability: 0.0,
        };
        let always = EpidemicConfig {
            forward_probability: 1.0,
        };

        for _ in 0..1000 {
            assert!(!never.should_forward(), "0.0 disables forwarding");
            assert!(always.should_forward(), "1.0 always forwards");
        }
    }
}
