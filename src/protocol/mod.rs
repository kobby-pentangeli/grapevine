//! Gossip protocol implementations.

pub mod anti_entropy;
pub mod epidemic;
pub mod gossip;

pub use anti_entropy::{AntiEntropy, AntiEntropyConfig, MessageEntry};
pub use epidemic::EpidemicConfig;
pub use gossip::Gossip;
