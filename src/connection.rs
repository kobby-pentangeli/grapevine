use message_io::network::Endpoint;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, iter::Iterator, net::SocketAddr};

/// Types of p2p messages
#[derive(Debug, Clone, Serialize, Deserialize)]
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

enum NodeInfo {
    OldInfo,
    NewInfo(SocketAddr),
}

/// Structure of a peer address
pub struct NodeAddr {
    /// Node's own public address
    pub public: SocketAddr,
    /// A peer's remote connection identity
    pub endpoint: Endpoint,
}

/// Structure of the map of peers in the network
pub struct NodeMap {
    map: HashMap<Endpoint, NodeInfo>,
    self_pub_addr: SocketAddr,
}

impl NodeMap {
    /// Creates a new `NodeMap`
    pub fn new(self_pub_addr: SocketAddr) -> Self {
        Self {
            map: HashMap::new(),
            self_pub_addr,
        }
    }

    /// Adds an old info on the node
    pub fn add_old_one(&mut self, endpoint: Endpoint) {
        self.map.insert(endpoint, NodeInfo::OldInfo);
    }

    /// Adds a new info on the node
    pub fn add_new_one(&mut self, endpoint: Endpoint, pub_addr: SocketAddr) {
        self.map.insert(endpoint, NodeInfo::NewInfo(pub_addr));
    }

    /// Removes a node's endpoint from the map
    pub fn drop(&mut self, endpoint: Endpoint) {
        self.map.remove(&endpoint);
    }

    /// Retrieves the list of peers in the network
    pub fn get_peers_list(&self) -> Vec<SocketAddr> {
        let mut list: Vec<SocketAddr> = Vec::with_capacity(self.map.len() + 1);
        list.push(self.self_pub_addr);
        self.map
            .iter()
            .map(|(endpoint, info)| match info {
                NodeInfo::OldInfo => endpoint.addr(),
                NodeInfo::NewInfo(public_addr) => *public_addr,
            })
            .for_each(|addr| {
                list.push(addr);
            });

        list
    }

    /// Retrieves peer addresses
    pub fn fetch_receivers(&self) -> Vec<NodeAddr> {
        self.map
            .iter()
            .map(|(endpoint, info)| {
                let public = match info {
                    NodeInfo::OldInfo => endpoint.addr(),
                    NodeInfo::NewInfo(public_addr) => *public_addr,
                };
                NodeAddr {
                    endpoint: *endpoint,
                    public,
                }
            })
            .collect()
    }

    /// Retrieves the public address of the node
    pub fn get_pub_addr(&self, endpoint: &Endpoint) -> Option<SocketAddr> {
        self.map.get(endpoint).map(|info| match info {
            NodeInfo::OldInfo => endpoint.addr(),
            NodeInfo::NewInfo(addr) => *addr,
        })
    }
}
