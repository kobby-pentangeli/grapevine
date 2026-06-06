//! Core gossip protocol engine.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use rand::seq::SliceRandom;
use tokio::sync::broadcast;
use tokio::time::{self, sleep};
use tracing::{debug, info, trace, warn};

use crate::{
    AntiEntropy, EpidemicConfig, Error, Message, MessageEntry, MessageId, NodeConfig, Payload,
    Peer, PeerId, PeerState, Result, Tcp,
};

/// Shutdown broadcast channel capacity.
const SHUTDOWN_CHANNEL_CAPACITY: usize = 16;

/// Grace period for in-flight messages during shutdown (milliseconds).
const SHUTDOWN_GRACE_PERIOD_MS: u64 = 500;

/// Peer maintenance check interval (seconds).
const PEER_MAINTENANCE_INTERVAL_SECS: u64 = 10;

/// Message deduplication cleanup interval (seconds).
const MESSAGE_CLEANUP_INTERVAL_SECS: u64 = 30;

/// Main gossip protocol engine.
pub struct Gossip {
    /// Node configuration
    config: NodeConfig,

    /// TCP transport
    transport: Arc<Tcp>,

    /// Connected peers
    peers: Arc<DashMap<PeerId, Peer>>,

    /// Seen messages with full message data and metadata
    seen_messages: Arc<DashMap<MessageId, MessageEntry>>,

    /// Maps canonical peer addresses (listening addresses) to connection addresses (ephemeral ports).
    /// When peer A connects to peer B, B sees the connection from an ephemeral port, but messages
    /// contain A's listening address in message.id.origin. This map allows us to look up the
    /// correct connection to use when sending direct messages.
    canonical_addrs: Arc<DashMap<SocketAddr, SocketAddr>>,

    /// Application message handler, set once before the node starts.
    message_handler: OnceLock<Arc<dyn Fn(SocketAddr, Bytes) + Send + Sync>>,

    /// Shutdown signal broadcaster
    shutdown_tx: broadcast::Sender<()>,

    /// Anti-entropy engine
    anti_entropy: Option<Arc<AntiEntropy>>,

    /// Epidemic broadcast config
    epidemic_config: EpidemicConfig,
}

impl Gossip {
    /// Create a new gossip protocol instance.
    ///
    /// # Errors
    /// Returns [`Error::Config`] if rate limiting is enabled with an invalid
    /// capacity or refill rate.
    pub fn new(config: NodeConfig) -> Result<Self> {
        let (shutdown_tx, _) = broadcast::channel(SHUTDOWN_CHANNEL_CAPACITY);

        let mut transport = Tcp::with_max_message_size(config.max_message_size);
        if config.rate_limit.enabled {
            transport = transport
                .set_rate_limit(config.rate_limit.capacity, config.rate_limit.refill_rate)?;
        }
        let transport = Arc::new(transport);
        let seen_messages = Arc::new(DashMap::new());
        let epidemic_config = config.epidemic.clone();

        let anti_entropy = if config.anti_entropy.enabled {
            Some(Arc::new(AntiEntropy::new(
                config.anti_entropy.clone(),
                Arc::clone(&transport),
                Arc::clone(&seen_messages),
            )))
        } else {
            None
        };

        Ok(Self {
            config,
            transport,
            peers: Arc::new(DashMap::new()),
            seen_messages,
            canonical_addrs: Arc::new(DashMap::new()),
            message_handler: OnceLock::new(),
            shutdown_tx,
            anti_entropy,
            epidemic_config,
        })
    }

    /// Set the application message handler.
    ///
    /// The handler is captured when the node starts, so it must be set before
    /// [`Gossip::start`]; a second call has no effect.
    pub fn set_message_handler<F>(&self, handler: F)
    where
        F: Fn(SocketAddr, Bytes) + Send + Sync + 'static,
    {
        let _ = self.message_handler.set(Arc::new(handler));
    }

    /// Start the gossip protocol.
    pub async fn start(&self) -> Result<()> {
        self.transport.listen(self.config.bind_addr).await?;
        let local_addr = self
            .transport
            .local_addr()
            .ok_or_else(|| Error::internal("Transport has no local address after listening"))?;
        info!("Gossip node started on {local_addr}");

        for peer in &self.config.bootstrap_peers {
            if let Err(e) = self.connect_to_peer(*peer).await {
                warn!("Failed to connect to bootstrap peer {peer}: {e}");
            }
        }

        self.spawn_message_receiver();
        self.spawn_gossip_loop();
        self.spawn_peer_maintenance();
        self.spawn_message_cleanup();

        if let Some(ref anti_entropy) = self.anti_entropy {
            anti_entropy.start().await?;
        }

        Ok(())
    }

    /// Connect to a peer.
    pub async fn connect_to_peer(&self, addr: SocketAddr) -> Result<()> {
        let transport = &self.transport;
        transport.connect(addr).await?;

        let local_addr = transport
            .local_addr()
            .ok_or_else(|| Error::internal("No local address"))?;

        // Send immediate heartbeat to establish canonical address mapping
        let heartbeat = Message::new(local_addr, Payload::Heartbeat { from: local_addr });
        transport.send(addr, heartbeat).await?;

        // Request peer list
        let message = Message::new(local_addr, Payload::PeerListRequest);
        transport.send(addr, message).await?;

        info!("Connected to peer {addr}");
        Ok(())
    }

    /// Broadcast a message to the network.
    pub async fn broadcast(&self, data: Bytes) -> Result<()> {
        let local_addr = self
            .transport
            .local_addr()
            .ok_or_else(|| Error::internal("No local address"))?;

        let message = Message::new(local_addr, Payload::Application(data));

        self.seen_messages.insert(
            message.id,
            MessageEntry {
                message: message.clone(),
                first_seen: Instant::now(),
                forward_count: 0,
            },
        );

        self.gossip_message(message).await
    }

    /// Send a direct message to a specific peer.
    ///
    /// The `peer` parameter should be the peer's canonical listening address.
    /// This method will automatically resolve it to the actual connection address.
    pub async fn send_to_peer(&self, peer: SocketAddr, data: Bytes) -> Result<()> {
        let local_addr = self
            .transport
            .local_addr()
            .ok_or_else(|| Error::internal("No local address"))?;

        // Resolve canonical address to connection address.
        // If the peer is us (we're listening on `peer`), use peer directly.
        // Otherwise, look up the connection address from our canonical mapping.
        let connection_addr = self
            .canonical_addrs
            .get(&peer)
            .map(|entry| *entry.value())
            .unwrap_or(peer);

        // Check if peer is connected using the transport's peer list
        if !self.transport.peers().contains(&connection_addr) {
            return Err(Error::PeerNotFound(peer));
        }

        let message = Message::new(
            local_addr,
            Payload::DirectMessage {
                recipient: peer,
                data,
            },
        );

        // Mark as seen to avoid processing it if it comes back
        self.seen_messages.insert(
            message.id,
            MessageEntry {
                message: message.clone(),
                first_seen: Instant::now(),
                forward_count: 0,
            },
        );

        // Send to the connection address, not the canonical address
        self.transport.send(connection_addr, message).await
    }

    /// Get local address.
    pub async fn local_addr(&self) -> Option<SocketAddr> {
        self.transport.local_addr()
    }

    /// Get list of connected peers using their canonical (listening) addresses.
    ///
    /// This returns the peers' actual listening addresses rather than the ephemeral
    /// connection ports. Use these addresses for sending direct messages.
    pub async fn peer_list(&self) -> Vec<SocketAddr> {
        let connection_addrs = self.transport.peers();

        // Build reverse mapping: connection_addr -> canonical_addr
        let mut reverse_map: HashMap<SocketAddr, SocketAddr> = HashMap::new();

        for entry in self.canonical_addrs.iter() {
            let (canonical, connection) = (*entry.key(), *entry.value());
            reverse_map.insert(connection, canonical);
        }

        // Return canonical addresses where available, connection addresses otherwise
        connection_addrs
            .into_iter()
            .map(|conn_addr| reverse_map.get(&conn_addr).copied().unwrap_or(conn_addr))
            .collect()
    }

    /// Shutdown the node gracefully.
    pub async fn shutdown(&self) -> Result<()> {
        info!("Initiating graceful shutdown");

        let local_addr = self.transport.local_addr();
        if let Some(addr) = local_addr {
            let goodbye = Message::new(
                addr,
                Payload::Goodbye {
                    reason: "Normal shutdown".to_string(),
                },
            );

            let peer_addrs = self.transport.peers();
            debug!("Sending goodbye to {} peers", peer_addrs.len());

            for peer_addr in peer_addrs {
                if let Err(e) = self.transport.send(peer_addr, goodbye.clone()).await {
                    debug!("Failed to send goodbye to {peer_addr}: {e}");
                }
            }
        }

        debug!("Broadcasting shutdown signal");
        let _ = self.shutdown_tx.send(());

        sleep(Duration::from_millis(SHUTDOWN_GRACE_PERIOD_MS)).await;

        debug!("Clearing peer list");
        self.peers.clear();

        debug!("Clearing message cache");
        self.seen_messages.clear();

        info!("Graceful shutdown complete");
        Ok(())
    }

    fn spawn_message_receiver(&self) {
        let transport = Arc::clone(&self.transport);
        let peers = Arc::clone(&self.peers);
        let seen_messages = Arc::clone(&self.seen_messages);
        let canonical_addrs = Arc::clone(&self.canonical_addrs);
        let message_handler = self.message_handler.get().cloned();
        let config = self.config.clone();
        let epidemic_config = self.epidemic_config.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            loop {
                let recv_fut = transport.recv();

                let result = tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("Message receiver shutting down");
                        return;
                    }
                    result = recv_fut => result,
                };

                let (peer_addr, message) = match result {
                    Ok(msg) => msg,
                    Err(e) => {
                        warn!("Error receiving message: {e}");
                        continue;
                    }
                };

                let local_addr = match transport.local_addr() {
                    Some(addr) => addr,
                    None => continue,
                };

                // Map the canonical address (from message.id.origin) to the connection address (peer_addr).
                // This allows us to send direct messages using the peer's listening address.
                // We always create the mapping, even if addresses match (outbound connections).
                let canonical_addr = message.id.origin;
                canonical_addrs.insert(canonical_addr, peer_addr);
                if canonical_addr != peer_addr {
                    debug!("Mapped canonical {canonical_addr} -> connection {peer_addr}");
                } else {
                    debug!("Mapped canonical {canonical_addr} -> self (outbound connection)");
                }

                trace!("Received message from {peer_addr}: {:?}", message.id);

                match &message.payload {
                    Payload::PeerListRequest => {
                        Self::handle_peer_list_request(&transport, peer_addr).await;
                    }
                    Payload::PeerListResponse { peers: peer_list } => {
                        Self::handle_peer_list_response(&transport, peer_list).await;
                    }
                    Payload::AntiEntropyDigest { message_ids } => {
                        let _ = AntiEntropy::handle_digest(
                            local_addr,
                            peer_addr,
                            message_ids.clone(),
                            &transport,
                            &seen_messages,
                        )
                        .await;
                    }
                    Payload::MessageRequest { ids } => {
                        let _ = AntiEntropy::handle_message_request(
                            local_addr,
                            peer_addr,
                            ids.clone(),
                            &transport,
                            &seen_messages,
                        )
                        .await;
                    }
                    Payload::MessageResponse { messages: msgs } => {
                        AntiEntropy::handle_message_response(
                            msgs.clone(),
                            &seen_messages,
                            &message_handler,
                        );
                    }
                    Payload::Goodbye { reason } => {
                        // Use canonical address (listening address) in logs instead of connection address
                        let canonical_addr = message.id.origin;
                        info!("Peer {canonical_addr} is leaving: {reason}");
                        if let Some(peer_id) = peers
                            .iter()
                            .find(|p| p.value().id().0 == peer_addr)
                            .map(|p| *p.key())
                        {
                            peers.remove(&peer_id);
                            debug!("Removed peer {canonical_addr} from peer list");
                        }
                    }
                    Payload::DirectMessage { recipient, data } => {
                        // Direct messages are only delivered to the intended recipient
                        // and are not gossiped to other peers
                        if seen_messages.contains_key(&message.id) {
                            trace!("Duplicate direct message {}, ignoring", message.id);
                            continue;
                        }

                        // Check if we are the intended recipient
                        if *recipient == local_addr {
                            seen_messages.insert(
                                message.id,
                                MessageEntry {
                                    message: message.clone(),
                                    first_seen: Instant::now(),
                                    forward_count: 0,
                                },
                            );

                            if let Some(ref handler) = message_handler {
                                handler(message.id.origin, data.clone());
                            }
                            debug!("Received direct message from {}", message.id.origin);
                        } else {
                            // Not for us, drop immediately without storing to prevent DOS attacks.
                            // Direct messages are point-to-point and should not be relayed,
                            // so there's no need to track them if they're misdirected.
                            trace!(
                                "Direct message {} not for us (intended for {}), dropping",
                                message.id, recipient
                            );
                        }
                    }
                    Payload::Application(data) => {
                        if seen_messages.contains_key(&message.id) {
                            trace!("Duplicate message {}, ignoring", message.id);
                            continue;
                        }

                        seen_messages.insert(
                            message.id,
                            MessageEntry {
                                message: message.clone(),
                                first_seen: Instant::now(),
                                forward_count: 0,
                            },
                        );

                        if let Some(ref handler) = message_handler {
                            handler(message.id.origin, data.clone());
                        }

                        if message.ttl > 1 {
                            if !epidemic_config.should_forward() {
                                trace!("Epidemic broadcast: not forwarding message {}", message.id);
                                continue;
                            }

                            if let Some(mut entry) = seen_messages.get_mut(&message.id) {
                                if entry.forward_count >= epidemic_config.max_forwards {
                                    trace!(
                                        "Message {} reached max forwards ({})",
                                        message.id, epidemic_config.max_forwards
                                    );
                                    continue;
                                }
                                entry.forward_count += 1;
                            }

                            let mut new_message = message.clone();
                            new_message.decrement_ttl();
                            let _ = Self::gossip_to_fanout(
                                &transport,
                                &peers,
                                new_message,
                                config.fanout,
                            )
                            .await;
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    fn spawn_gossip_loop(&self) {
        let interval = self.config.gossip_interval;
        let transport = Arc::clone(&self.transport);
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("Gossip loop shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        let local_addr = match transport.local_addr() {
                            Some(addr) => addr,
                            None => continue,
                        };

                        let heartbeat = Message::new(local_addr, Payload::Heartbeat { from: local_addr });
                        let peer_addrs = transport.peers();

                        for peer_addr in peer_addrs {
                            if let Err(e) = transport.send(peer_addr, heartbeat.clone()).await {
                                debug!("Failed to send heartbeat to {peer_addr}: {e}");
                            }
                        }
                    }
                }
            }
        });
    }

    fn spawn_peer_maintenance(&self) {
        let peers = Arc::clone(&self.peers);
        let timeout = self.config.peer_timeout;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = time::interval(Duration::from_secs(PEER_MAINTENANCE_INTERVAL_SECS));
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("Peer maintenance shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        let mut peers_to_mark_stale = Vec::new();
                        let mut peers_to_disconnect = Vec::new();

                        for entry in peers.iter() {
                            let peer_info = &entry.value().info;

                            if peer_info.should_disconnect() {
                                peers_to_disconnect.push(*entry.key());
                            } else if peer_info.is_stale(timeout) && peer_info.state != PeerState::Stale {
                                peers_to_mark_stale.push(*entry.key());
                            }
                        }

                        for peer_id in peers_to_mark_stale {
                            if let Some(mut peer) = peers.get_mut(&peer_id) {
                                peer.info.mark_stale();
                                debug!("Marked peer {peer_id} as stale");
                            }
                        }

                        for peer_id in peers_to_disconnect {
                            info!("Disconnecting unhealthy peer {peer_id}");
                            peers.remove(&peer_id);
                        }
                    }
                }
            }
        });
    }

    fn spawn_message_cleanup(&self) {
        let seen_messages = Arc::clone(&self.seen_messages);
        let ttl = self.config.message_dedup_ttl;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = time::interval(Duration::from_secs(MESSAGE_CLEANUP_INTERVAL_SECS));
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("Message cleanup shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        let now = Instant::now();
                        let stale_messages: Vec<MessageId> = seen_messages
                            .iter()
                            .filter(|entry| now.duration_since(entry.value().first_seen) > ttl)
                            .map(|entry| *entry.key())
                            .collect();

                        let count = stale_messages.len();
                        for message_id in stale_messages {
                            seen_messages.remove(&message_id);
                        }

                        if count > 0 {
                            debug!("Cleaned up {count} stale message IDs");
                        }
                    }
                }
            }
        });
    }

    async fn gossip_message(&self, message: Message) -> Result<()> {
        Self::gossip_to_fanout(&self.transport, &self.peers, message, self.config.fanout).await
    }

    async fn gossip_to_fanout(
        transport: &Arc<Tcp>,
        _peers: &Arc<DashMap<PeerId, Peer>>,
        message: Message,
        fanout: usize,
    ) -> Result<()> {
        let mut peer_addrs = transport.peers();

        if peer_addrs.is_empty() {
            return Ok(());
        }

        let selected: Vec<_> = {
            let mut rng = rand::rng();
            peer_addrs.shuffle(&mut rng);
            peer_addrs.into_iter().take(fanout).collect()
        };

        for addr in selected {
            if let Err(e) = transport.send(addr, message.clone()).await {
                warn!("Failed to gossip to {addr}: {e}");
            }
        }

        Ok(())
    }

    async fn handle_peer_list_request(transport: &Arc<Tcp>, sender: SocketAddr) {
        let peer_list = transport.peers();

        let local_addr = match transport.local_addr() {
            Some(addr) => addr,
            None => return,
        };

        let response = Message::new(local_addr, Payload::PeerListResponse { peers: peer_list });
        if let Err(e) = transport.send(sender, response).await {
            warn!("Failed to send peer list to {sender}: {e}");
        }
    }

    async fn handle_peer_list_response(transport: &Arc<Tcp>, peer_list: &[SocketAddr]) {
        for &peer_addr in peer_list {
            if let Err(e) = transport.connect(peer_addr).await {
                debug!("Failed to connect to peer {peer_addr}: {e}");
            }
        }
    }
}
