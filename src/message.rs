//! Nature of p2p messages

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Types of p2p messages
#[derive(Serialize, Deserialize)]
pub enum Message {
    /// Peer's public address
    RetrievePubAddr(SocketAddr),
    /// Request for list of peers
    RetrievePeerList,
    /// Response for RetrievePeerList with peers info
    RespondToListQuery(Vec<SocketAddr>),
    /// Some random message
    RequestRandomInfo(String),
}
