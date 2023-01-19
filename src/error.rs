/// Error types
#[derive(Debug, Clone, thiserror::Error)]
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

    /// Error on deserializing input data
    #[error("{0}")]
    BincodeDeserializeError(String),

    /// Error on serializing input data
    #[error("{0}")]
    BincodeSerializeError(String),

    /// Error on adding a peer to the peer list
    #[error("{0}")]
    AddPeerError(String),

    /// Error on logging
    #[error("{0}")]
    LoggingError(String),
}
