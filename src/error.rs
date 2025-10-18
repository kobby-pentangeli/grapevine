//! Error types for Grapevine.

use std::io;
use std::net::SocketAddr;

/// Result type alias for all operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for all operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Network operation failed.
    #[error("Network error: {message}")]
    Network {
        /// Error message
        message: String,
        /// Associated I/O error if available
        #[source]
        source: Option<io::Error>,
    },

    /// Failed to bind to address or connect to peer.
    #[error("Failed to connect/bind to {addr}: {source}")]
    Connection {
        /// The target address
        addr: SocketAddr,
        /// The underlying error
        #[source]
        source: io::Error,
    },

    /// (De)serialization errors.
    #[error("(De)serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    Config(String),

    /// Peer not found.
    #[error("Peer not found: {0}")]
    PeerNotFound(SocketAddr),

    /// Node is shutting down.
    #[error("Node is shutting down")]
    Shutdown,

    /// Message too large.
    #[error("Message size {size} exceeds maximum {max}")]
    MessageTooLarge {
        /// Actual message size
        size: usize,
        /// Maximum allowed size
        max: usize,
    },

    /// Invalid message format.
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    /// Channel send/receive error.
    #[error("Channel send/receive error: {0}")]
    Channel(String),

    /// Timeout occurred.
    #[error("Operation timed out after {duration_ms}ms")]
    Timeout {
        /// Duration in milliseconds
        duration_ms: u64,
    },

    /// Cryptographic operation failed.
    #[cfg(feature = "crypto")]
    #[error("Cryptographic error: {0}")]
    Crypto(String),

    /// Invalid signature.
    #[cfg(feature = "crypto")]
    #[error("Invalid signature from peer {0}")]
    InvalidSignature(SocketAddr),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Create a network error with a message.
    pub fn network(message: impl Into<String>) -> Self {
        Self::Network {
            message: message.into(),
            source: None,
        }
    }

    /// Create a network error with source.
    pub fn network_with_source(message: impl Into<String>, source: io::Error) -> Self {
        Self::Network {
            message: message.into(),
            source: Some(source),
        }
    }

    /// Create an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

/// Create internal errors with file/line information.
#[macro_export]
macro_rules! internal_error {
    ($msg:expr) => {
        $crate::error::Error::internal(format!("{} at {}:{}", $msg, file!(), line!()))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::internal(format!("{} at {}:{}", format!($fmt, $($arg)*), file!(), line!()))
    };
}
