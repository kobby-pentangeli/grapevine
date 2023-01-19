use crate::connection::{Message, NodeAddr, NodeMap};
use message_io::{
    events::EventQueue,
    network::{Endpoint, NetEvent, Network, Transport},
};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Structure of a node
pub struct Node {
    /// List of peers of the current node
    pub connections: Arc<Mutex<NodeMap>>,
    /// Public address of the node
    pub node_addr: SocketAddr,
    /// Sets the duration (in seconds) of
    /// emitting messages to other peers
    pub duration: u32,
    /// Network of nodes
    pub network: Arc<Mutex<Network>>,
    /// Network events queue
    pub event_queue: EventQueue<NetEvent>,
    /// Sets the optional peer address to connect to
    pub peer: Option<String>,
}

impl Node {
    /// Creates a new `Node`
    pub fn new(port: u32, duration: u32, peer: Option<String>) -> Result<Self, String> {
        let (mut network, event_queue) = Network::split();

        // Node's own listening address (localhost + port)
        let listening_addr = format!("127.0.0.1:{}", port);
        match network.listen(Transport::FramedTcp, &listening_addr) {
            Ok((_, addr)) => {
                log_my_address(&addr);

                Ok(Self {
                    connections: Arc::new(Mutex::new(NodeMap::new(addr))),
                    node_addr: addr,
                    duration,
                    network: Arc::new(Mutex::new(network)),
                    event_queue,
                    peer,
                })
            }
            Err(_) => Err(format!("Cannot listen on {}", listening_addr)),
        }
    }

    /// Executes the peer-to-peer process.
    pub fn execute(mut self) {
        if let Some(addr) = &self.peer {
            let mut network = self.network.lock().expect("Failed to lock network");

            // Connection to the first peer
            match network.connect(Transport::FramedTcp, addr) {
                Ok((endpoint, _)) => {
                    {
                        let mut nodes = self.connections.lock().expect("Unable to fetch peer list");
                        nodes.add_old_one(endpoint);
                    }

                    send_message(
                        &mut network,
                        endpoint,
                        &Message::RetrievePubAddr(self.node_addr),
                    );

                    // Request a list of existing peers
                    // Response will be in event queue
                    send_message(&mut network, endpoint, &Message::RetrievePeerList);
                }
                Err(_) => {
                    println!("Failed to connect to {}", &addr);
                }
            }
        }

        // spawning thread which will be send random messages to known peers
        self.spawn_emit_loop();

        loop {
            match self.event_queue.receive() {
                // Waiting events
                NetEvent::Message(message_sender, input_data) => {
                    match bincode::deserialize(&input_data)
                        .expect("Failed to deserialize input data")
                    {
                        Message::RetrievePubAddr(pub_addr) => {
                            let mut peers = self
                                .connections
                                .lock()
                                .expect("Error in retrieving peer list");
                            peers.add_new_one(message_sender, pub_addr);
                        }
                        Message::RetrievePeerList => {
                            let list = {
                                let peers = self.connections.lock().expect("Unable to fetch peers");
                                peers.get_peers_list()
                            };
                            let msg = Message::RespondToListQuery(list);
                            send_message(
                                &mut self.network.lock().expect("Error in sending message"),
                                message_sender,
                                &msg,
                            );
                        }
                        Message::RespondToListQuery(addrs) => {
                            let filtered: Vec<&SocketAddr> =
                                addrs.iter().filter(|x| *x != &self.node_addr).collect();

                            log_connected_to_the_peers(&filtered);

                            let mut network = self.network.lock().expect("Unable to lock network");

                            for peer in filtered {
                                if peer == &message_sender.addr() {
                                    continue;
                                }

                                // connecting to peer
                                let (endpoint, _) = network
                                    .connect(Transport::FramedTcp, *peer)
                                    .expect("Error in connecting to peer");

                                // sending public address
                                let msg = Message::RetrievePubAddr(self.node_addr);
                                send_message(&mut network, endpoint, &msg);

                                // saving peer
                                self.connections
                                    .lock()
                                    .expect("Error in saving peer")
                                    .add_old_one(endpoint);
                            }
                        }
                        Message::RequestRandomInfo(text) => {
                            let pub_addr = self
                                .connections
                                .lock()
                                .expect("Error in fetching peers")
                                .get_pub_addr(&message_sender)
                                .expect("Error in fetching public address");
                            log_message_received(&pub_addr, &text);
                        }
                    }
                }
                NetEvent::Connected(_, _) => {}
                NetEvent::Disconnected(endpoint) => {
                    let mut peers = self.connections.lock().expect("Unable to fetch peer list");
                    NodeMap::drop(&mut peers, endpoint);
                }
            }
        }
    }

    fn spawn_emit_loop(&self) {
        let sleep_duration = Duration::from_secs(self.duration as u64);
        let peers_mut = Arc::clone(&self.connections);
        let network_mut = Arc::clone(&self.network);

        std::thread::spawn(move || {
            // sleeping and sending
            loop {
                std::thread::sleep(sleep_duration);

                let peers = peers_mut.lock().expect("Unable to lock peers");
                let receivers = peers.fetch_receivers();

                // if there are no receivers, skip
                if receivers.is_empty() {
                    continue;
                }

                let mut network = network_mut.lock().expect("Failed to lock network");

                let msg_text = generate_random_message();
                let msg = Message::RequestRandomInfo(msg_text.clone());

                log_sending_message(
                    &msg_text,
                    &receivers
                        .iter()
                        .map(|NodeAddr { public, .. }| public)
                        .collect(),
                );

                for NodeAddr { endpoint, .. } in receivers {
                    send_message(&mut network, endpoint, &msg);
                }
            }
        });
    }
}

fn send_message(network: &mut Network, to: Endpoint, msg: &Message) {
    let output_data = bincode::serialize(msg).expect("Failed to serialize message");
    network.send(to, &output_data);
}

fn generate_random_message() -> String {
    petname::Petnames::default().generate_one(2, "-")
}

trait ToSocketAddr {
    fn get_addr(&self) -> SocketAddr;
}

impl ToSocketAddr for Endpoint {
    fn get_addr(&self) -> SocketAddr {
        self.addr()
    }
}

impl ToSocketAddr for &Endpoint {
    fn get_addr(&self) -> SocketAddr {
        self.addr()
    }
}

impl ToSocketAddr for SocketAddr {
    fn get_addr(&self) -> SocketAddr {
        *self
    }
}

impl ToSocketAddr for &SocketAddr {
    fn get_addr(&self) -> SocketAddr {
        **self
    }
}

fn format_list_of_addrs<T: ToSocketAddr>(items: &Vec<T>) -> String {
    if items.is_empty() {
        "[no one]".to_owned()
    } else {
        let joined = items
            .iter()
            .map(|x| format!("\"{}\"", ToSocketAddr::get_addr(x)))
            .collect::<Vec<String>>()
            .join(", ");

        format!("[{}]", joined)
    }
}

fn log_message_received<T: ToSocketAddr>(from: &T, text: &str) {
    log::info!(
        "Received message [{}] from \"{}\"",
        text,
        ToSocketAddr::get_addr(from)
    );
}

fn log_my_address<T: ToSocketAddr>(addr: &T) {
    log::info!("My address is \"{}\"", ToSocketAddr::get_addr(addr));
}

fn log_connected_to_the_peers<T: ToSocketAddr>(peers: &Vec<T>) {
    log::info!("Connected to the peers at {}", format_list_of_addrs(peers));
}

fn log_sending_message<T: ToSocketAddr>(message: &str, receivers: &Vec<T>) {
    log::info!(
        "Sending message [{}] to {}",
        message,
        format_list_of_addrs(receivers)
    );
}
