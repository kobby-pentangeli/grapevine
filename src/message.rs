use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Serialize, Deserialize)]
pub enum Message {
    /// Peer's public address
    MyPubAddr(SocketAddr),
    /// Request for list of peers
    GiveMeAListOfPeers,
    /// Response for GiveMeAListOfPeers with peers info
    TakePeersList(Vec<SocketAddr>),
    /// Some random message
    Info(String),
}
