//! Anti-entropy protocol for ensuring message delivery.
//!
//! Periodically exchanges message digests with peers to detect
//! and repair missing messages.

use std::collections::HashSet;
use std::time::Duration;

use crate::core::MessageId;

/// Anti-entropy configuration.
#[derive(Debug, Clone)]
pub struct AntiEntropyConfig {
    /// Interval between anti-entropy rounds
    pub interval: Duration,

    /// Number of peers to sync with per round
    pub fanout: usize,
}

impl Default for AntiEntropyConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            fanout: 3,
        }
    }
}

/// Represents a digest of known messages.
#[derive(Debug, Clone)]
pub struct MessageDigest {
    /// Set of known message IDs
    pub message_ids: HashSet<MessageId>,
}

impl MessageDigest {
    /// Create a new empty digest.
    pub fn new() -> Self {
        Self {
            message_ids: HashSet::new(),
        }
    }

    /// Add a message ID to the digest.
    pub fn add(&mut self, id: MessageId) {
        self.message_ids.insert(id);
    }

    /// Compute the difference between this digest and another.
    pub fn diff(&self, other: &MessageDigest) -> Vec<MessageId> {
        other
            .message_ids
            .difference(&self.message_ids)
            .copied()
            .collect()
    }
}

impl Default for MessageDigest {
    fn default() -> Self {
        Self::new()
    }
}
