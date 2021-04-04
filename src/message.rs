use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Serialize, Deserialize)]
pub enum Message {
    MyPubAddr(SocketAddr),

    // Request for list of peers
    GiveMeAListOfPeers,

    // Response for GiveMeAListOfPeers with peers info
    TakePeersList(Vec<SocketAddr>),

    // Some random message
    Info(String),
}

// impl Message {
//     // serialization by bincode

//     pub fn ser(&self) -> Result<Vec<u8>> {
//         bincode::serialize(self)
//     }

//     pub fn de(data: &Vec<u8>) -> Result<Self> {
//         bincode::deserialize(data)
//     }
// }