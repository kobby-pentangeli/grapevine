//! Functionality for map of peers and related information

use message_io::network::Endpoint;
use std::collections::HashMap;
use std::iter::Iterator;
use std::net::SocketAddr;

/// Structure of the map of peers in the network
pub struct NodeMap {
    map: HashMap<Endpoint, NodeInfo>,
    self_pub_addr: SocketAddr,
}

/// Structure of a peer address
pub struct NodeAddr {
    /// Public address
    pub public: SocketAddr,
    /// SendTo address
    pub endpoint: Endpoint,
}

/// Information on a peer
enum NodeInfo {
    OldInfo,
    NewInfo(SocketAddr),
}

impl NodeMap {
    /// Generates a new node map
    pub fn new(self_pub_addr: SocketAddr) -> Self {
        Self {
            map: HashMap::new(),
            self_pub_addr,
        }
    }

    /// Adds an old info on the node
    pub fn add_old_one(&mut self, endpoint: Endpoint) {
        // println!("add old one: {}", endpoint.addr());
        self.map.insert(endpoint, NodeInfo::OldInfo);
    }

    /// Adds a new info on the node
    pub fn add_new_one(&mut self, endpoint: Endpoint, pub_addr: SocketAddr) {
        // println!("add new one: {} ({})", endpoint.addr(), pub_addr);
        self.map.insert(endpoint, NodeInfo::NewInfo(pub_addr));
    }

    /// Removes a node's endpoint from the map
    pub fn drop(&mut self, endpoint: Endpoint) {
        // println!("drop: {}", endpoint.addr());
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
        self.map.get(endpoint).map(|founded| match founded {
            NodeInfo::OldInfo => endpoint.addr(),
            NodeInfo::NewInfo(addr) => *addr,
        })
    }
}
