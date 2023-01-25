/// Error types
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error on calling `message_io::Network::listen()`
    #[error("{0}")]
    NetworkListeningError(String),

    /// Error on acquiring a mutex on the network of nodes
    #[error("{0}")]
    NetworkLockError(String),

    /// Error on fetching peer list
    #[error("{0}")]
    ConnectionsFetchError(String),

    /// Error on connecting to the network
    #[error("{0}")]
    NetworkConnectionError(String),

    /// (De)serialization errors
    #[error("{0}")]
    BincodeError(bincode::Error),

    /// Error on adding a peer to the peer list
    #[error("{0}")]
    AddPeerError(String),

    /// Error on logging
    #[error("{0}")]
    LoggingError(log::SetLoggerError),
}

impl From<bincode::Error> for Error {
    fn from(value: bincode::Error) -> Self {
        Error::BincodeError(value)
    }
}

impl From<log::SetLoggerError> for Error {
    fn from(value: log::SetLoggerError) -> Self {
        Error::LoggingError(value)
    }
}
