//! Error types for Grapevine.

use std::io;
use std::net::SocketAddr;

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

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::error::EncodeError),

    /// Deserialization error.
    #[error("Deserialization failed: {0}")]
    Deserialization(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    Config(String),

    /// Peer not found.
    #[error("Peer not found: {0}")]
    PeerNotFound(SocketAddr),

    /// Message too large.
    #[error("Message size {size} exceeds maximum {max}")]
    MessageTooLarge {
        /// Actual message size
        size: usize,
        /// Maximum allowed size
        max: usize,
    },

    /// Channel send/receive error.
    #[error("Channel send/receive error: {0}")]
    Channel(String),

    /// Cryptographic operation failed.
    #[error("Cryptographic error: {0}")]
    Crypto(String),

    /// A message's signature was missing or did not verify against the public
    /// key it carried.
    #[error("Invalid signature on message claiming origin {0}")]
    InvalidSignature(SocketAddr),

    /// A message claimed an origin already pinned to a different public key: a
    /// spoofing attempt.
    #[error("Origin {0} is pinned to a different key (possible spoofing)")]
    OriginKeyMismatch(SocketAddr),

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Result;

    #[test]
    fn error_from_bincode() {
        let bad_data: &[u8] = &[255, 255, 255];
        let result: Result<(u32, usize)> =
            bincode::serde::decode_from_slice(bad_data, bincode::config::standard())
                .map_err(|e| Error::Deserialization(e.to_string()));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Deserialization(_)));
    }

    #[test]
    fn internal_error_macro() {
        let err = internal_error!("test error");
        assert!(matches!(err, Error::Internal(_)));
        let msg = format!("{}", err);
        assert!(msg.contains("test error"));
        assert!(msg.contains("error.rs")); // Should include file name
    }

    #[test]
    fn internal_error_macro_with_format() {
        let value = 42;
        let err = internal_error!("value is {}", value);
        assert!(matches!(err, Error::Internal(_)));
        let msg = format!("{err}");
        assert!(msg.contains("value is 42"));
        assert!(msg.contains("error.rs"));
    }

    #[test]
    fn error_source_chain() {
        use std::error::Error as StdError;

        let io_err = io::Error::other("inner error");
        let err = Error::network_with_source("outer error", io_err);
        assert!(err.source().is_some());
    }
}
