use message_io::network::Endpoint;
use std::collections::HashMap;
use std::iter::Iterator;
use std::net::SocketAddr;

pub struct NodeMap {
    map: HashMap<Endpoint, NodeInfo>,
    self_pub_addr: SocketAddr,
}

pub struct NodeAddr {
    pub public: SocketAddr,
    pub endpoint: Endpoint,
}

// #[derive(Debug)]
enum NodeInfo {
    OldOne,
    NewOne(SocketAddr),
}

impl NodeMap {
    pub fn new(self_pub_addr: SocketAddr) -> Self {
        Self {
            map: HashMap::new(),
            self_pub_addr,
        }
    }

    pub fn add_old_one(&mut self, endpoint: Endpoint) {
        // println!("add old one: {}", endpoint.addr());
        self.map.insert(endpoint, NodeInfo::OldOne);
    }

    pub fn add_new_one(&mut self, endpoint: Endpoint, pub_addr: SocketAddr) {
        // println!("add new one: {} ({})", endpoint.addr(), pub_addr);
        self.map.insert(endpoint, NodeInfo::NewOne(pub_addr));
    }

    pub fn drop(&mut self, endpoint: Endpoint) {
        // println!("drop: {}", endpoint.addr());
        self.map.remove(&endpoint);
    }

    pub fn get_peers_list(&self) -> Vec<SocketAddr> {
        let mut list: Vec<SocketAddr> = Vec::with_capacity(self.map.len() + 1);
        list.push(self.self_pub_addr);
        self.map
            .iter()
            .map(|(endpoint, info)| match info {
                NodeInfo::OldOne => endpoint.addr(),
                NodeInfo::NewOne(public_addr) => public_addr.clone(),
            })
            .for_each(|addr| {
                list.push(addr);
            });

        list
    }

    pub fn receivers(&self) -> Vec<NodeAddr> {
        self.map
            .iter()
            .map(|(endpoint, info)| {
                let public = match info {
                    NodeInfo::OldOne => endpoint.addr(),
                    NodeInfo::NewOne(public_addr) => public_addr.clone(),
                };
                NodeAddr {
                    endpoint: endpoint.clone(),
                    public,
                }
            })
            .collect()
    }

    pub fn get_pub_addr(&self, endpoint: &Endpoint) -> Option<SocketAddr> {
        self.map.get(endpoint).map(|founded| match founded {
            NodeInfo::OldOne => endpoint.addr(),
            NodeInfo::NewOne(addr) => addr.clone(),
        })
    }
}
