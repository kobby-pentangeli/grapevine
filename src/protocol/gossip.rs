//! Core gossip protocol engine.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use rand::seq::SliceRandom;
use tokio::sync::broadcast;
use tokio::time;
use tracing::{debug, info, trace, warn};

use crate::{
    AntiEntropy, EpidemicConfig, Error, Message, MessageEntry, MessageId, NodeConfig, Payload,
    PeerInfo, PeerState, Result, Tcp,
};

/// Maps a peer's canonical address to its connection address.
type ListeningAddrs = DashMap<SocketAddr, SocketAddr>;

/// Shutdown broadcast channel capacity.
const SHUTDOWN_CHANNEL_CAPACITY: usize = 16;

/// Peer maintenance check interval (seconds).
const PEER_MAINTENANCE_INTERVAL_SECS: u64 = 10;

/// Message deduplication cleanup interval (seconds).
const MESSAGE_CLEANUP_INTERVAL_SECS: u64 = 30;

/// Main gossip protocol engine.
pub struct Gossip {
    /// Node configuration
    config: NodeConfig,

    /// TCP transport, which owns the authoritative peer registry
    transport: Arc<Tcp>,

    /// Seen messages with full message data and metadata
    seen_messages: Arc<DashMap<MessageId, MessageEntry>>,

    /// Maps canonical peer addresses to connection addresses (ephemeral ports).
    /// When peer A connects to peer B, B sees the connection from an ephemeral port, but messages
    /// contain A's listening address in message.id.origin. This map allows us to look up the
    /// correct connection to use when sending direct messages.
    listening_addrs: Arc<ListeningAddrs>,

    /// Application message handler, set once before the node starts.
    message_handler: OnceLock<Arc<dyn Fn(SocketAddr, Bytes) + Send + Sync>>,

    /// Shutdown signal broadcaster
    shutdown_tx: broadcast::Sender<()>,

    /// Anti-entropy engine
    anti_entropy: Option<Arc<AntiEntropy>>,

    /// Epidemic broadcast config
    epidemic_config: EpidemicConfig,

    /// Monotonic per-origin sequence counter for this node's own broadcasts.
    sequence: AtomicU64,
}

impl Gossip {
    /// Create a new gossip protocol instance.
    ///
    /// # Errors
    /// Returns [`Error::Config`] if rate limiting is enabled with an invalid
    /// capacity or refill rate.
    pub fn new(config: NodeConfig) -> Result<Self> {
        let (shutdown_tx, _) = broadcast::channel(SHUTDOWN_CHANNEL_CAPACITY);

        let mut transport =
            Tcp::with_max_message_size(config.max_message_size).set_max_peers(config.max_peers);
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
            seen_messages,
            listening_addrs: Arc::new(DashMap::new()),
            message_handler: OnceLock::new(),
            shutdown_tx,
            anti_entropy,
            epidemic_config,
            sequence: AtomicU64::new(0),
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
        let heartbeat = Message::new(local_addr, 0, Payload::Heartbeat { from: local_addr });
        transport.send(addr, heartbeat).await?;

        // Request peer list
        let message = Message::new(local_addr, 0, Payload::PeerListRequest);
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

        let sequence = self.sequence.fetch_add(1, AtomicOrdering::Relaxed);
        let message = Message::new(local_addr, sequence, Payload::Application(data));

        self.seen_messages.insert(
            message.id,
            MessageEntry {
                message: message.clone(),
                first_seen: Instant::now(),
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
            .listening_addrs
            .get(&peer)
            .map(|entry| *entry.value())
            .unwrap_or(peer);

        // Check if peer is connected using the transport's peer list
        if !self.transport.peers().contains(&connection_addr) {
            return Err(Error::PeerNotFound(peer));
        }

        let message = Message::new(
            local_addr,
            0,
            Payload::DirectMessage {
                recipient: peer,
                data,
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

        for entry in self.listening_addrs.iter() {
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
                0,
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

        debug!("Stopping background tasks");
        let _ = self.shutdown_tx.send(());

        self.transport.shutdown().await;

        debug!("Clearing message cache");
        self.seen_messages.clear();

        info!("Graceful shutdown complete");
        Ok(())
    }

    fn spawn_message_receiver(&self) {
        let transport = Arc::clone(&self.transport);
        let seen_messages = Arc::clone(&self.seen_messages);
        let canonical_addrs = Arc::clone(&self.listening_addrs);
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
                    Payload::Heartbeat { from } => {
                        trace!("Heartbeat from {from}");
                    }
                    Payload::PeerListRequest => {
                        Self::handle_peer_list_request(&transport, &canonical_addrs, peer_addr)
                            .await;
                    }
                    Payload::PeerListResponse { peers: peer_list } => {
                        Self::handle_peer_list_response(
                            &transport,
                            &canonical_addrs,
                            local_addr,
                            peer_list,
                        )
                        .await;
                    }
                    Payload::AntiEntropyDigest { version_vector } => {
                        let _ = AntiEntropy::handle_digest(
                            local_addr,
                            peer_addr,
                            version_vector.clone(),
                            &transport,
                            &seen_messages,
                        )
                        .await;
                    }
                    Payload::MessageRequest { version_vector } => {
                        let _ = AntiEntropy::handle_message_request(
                            local_addr,
                            peer_addr,
                            version_vector.clone(),
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
                        let canonical_addr = message.id.origin;
                        info!("Peer {canonical_addr} is leaving: {reason}");
                        if transport.disconnect(peer_addr) {
                            canonical_addrs.retain(|_, conn| *conn != peer_addr);
                            debug!("Removed peer {canonical_addr} from registry");
                        }
                    }
                    Payload::DirectMessage { recipient, data } => {
                        if *recipient == local_addr {
                            if let Some(ref handler) = message_handler {
                                handler(message.id.origin, data.clone());
                            }
                            debug!("Received direct message from {}", message.id.origin);
                        } else {
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
                            },
                        );

                        if let Some(ref handler) = message_handler {
                            handler(message.id.origin, data.clone());
                        }

                        if message.ttl > 1 && epidemic_config.should_forward() {
                            let exclude =
                                fanout_exclusions(peer_addr, message.id.origin, &canonical_addrs);
                            let mut new_message = message.clone();
                            new_message.decrement_ttl();
                            let _ = Self::gossip_to_fanout(
                                &transport,
                                new_message,
                                config.fanout,
                                &exclude,
                            )
                            .await;
                        }
                    }
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

                        let heartbeat = Message::new(local_addr, 0, Payload::Heartbeat { from: local_addr });
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
        let transport = Arc::clone(&self.transport);
        let canonical_addrs = Arc::clone(&self.listening_addrs);
        let timeout = self.config.peer_timeout;
        let max_peers = self.config.max_peers;
        let interval = (timeout / 2).clamp(
            Duration::from_secs(1),
            Duration::from_secs(PEER_MAINTENANCE_INTERVAL_SECS),
        );
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("Peer maintenance shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        let infos = transport.peer_infos();
                        for action in plan_maintenance(&infos, Instant::now(), timeout, max_peers) {
                            match action {
                                MaintenanceAction::MarkStale(addr) => {
                                    transport.mark_stale(addr);
                                    debug!("Marked peer {addr} as stale");
                                }
                                MaintenanceAction::Disconnect(addr) => {
                                    if transport.disconnect(addr) {
                                        info!("Disconnected peer {addr}");
                                    }
                                }
                            }
                        }
                        let live: HashSet<SocketAddr> = transport.peers().into_iter().collect();
                        canonical_addrs.retain(|_, conn| live.contains(conn));
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
        Self::gossip_to_fanout(
            &self.transport,
            message,
            self.config.fanout,
            &HashSet::new(),
        )
        .await
    }

    /// Push `message` to up to `fanout` randomly selected peers, skipping any
    /// connection in `exclude`. The exclusion set carries the connection the
    /// message arrived on and the origin so a rumor is never echoed straight
    /// back to the node it came from.
    async fn gossip_to_fanout(
        transport: &Arc<Tcp>,
        message: Message,
        fanout: usize,
        exclude: &HashSet<SocketAddr>,
    ) -> Result<()> {
        let mut peer_addrs: Vec<SocketAddr> = transport
            .peers()
            .into_iter()
            .filter(|addr| !exclude.contains(addr))
            .collect();

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

    async fn handle_peer_list_request(
        transport: &Arc<Tcp>,
        canonical_addrs: &ListeningAddrs,
        sender: SocketAddr,
    ) {
        let Some(local_addr) = transport.local_addr() else {
            return;
        };

        let peers = known_canonical_peers(transport, canonical_addrs);
        let response = Message::new(local_addr, 0, Payload::PeerListResponse { peers });
        if let Err(e) = transport.send(sender, response).await {
            warn!("Failed to send peer list to {sender}: {e}");
        }
    }

    async fn handle_peer_list_response(
        transport: &Arc<Tcp>,
        canonical_addrs: &ListeningAddrs,
        local_addr: SocketAddr,
        peer_list: &[SocketAddr],
    ) {
        let connected: HashSet<SocketAddr> = transport.peers().into_iter().collect();

        for &peer in peer_list {
            let already = connected.contains(&peer)
                || canonical_addrs
                    .get(&peer)
                    .is_some_and(|conn| connected.contains(conn.value()));
            if peer == local_addr || already {
                continue;
            }

            if let Err(e) = transport.connect(peer).await {
                debug!("Failed to connect to advertised peer {peer}: {e}");
            }
        }
    }
}

/// Connections to skip when re-disseminating a received rumor
fn fanout_exclusions(
    sender: SocketAddr,
    origin: SocketAddr,
    canonical_addrs: &ListeningAddrs,
) -> HashSet<SocketAddr> {
    let mut exclude = HashSet::from([sender, origin]);
    if let Some(connection) = canonical_addrs.get(&origin) {
        exclude.insert(*connection.value());
    }
    exclude
}

fn known_canonical_peers(transport: &Tcp, canonical_addrs: &ListeningAddrs) -> Vec<SocketAddr> {
    let live: HashSet<SocketAddr> = transport.peers().into_iter().collect();
    canonical_addrs
        .iter()
        .filter(|entry| live.contains(entry.value()))
        .map(|entry| *entry.key())
        .collect()
}

/// A change the maintenance loop should apply to the peer registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaintenanceAction {
    /// Demote a peer that has gone quiet to `Stale`.
    MarkStale(SocketAddr),
    /// Evict a peer from the registry.
    Disconnect(SocketAddr),
}

/// Decide the maintenance actions for the current peer set.
///
/// A peer is disconnected when it has exhausted its consecutive-failure budget
/// or has been silent past twice the timeout (`Stale -> Disconnected`); it is
/// demoted to `Stale` after one timeout of silence (`Connected -> Stale`); and,
/// as a safety net should admission control ever be bypassed, the lowest
/// [`PeerInfo::health_score`] peers are evicted down to `max_peers`.
fn plan_maintenance(
    infos: &[(SocketAddr, PeerInfo)],
    now: Instant,
    peer_timeout: Duration,
    max_peers: usize,
) -> Vec<MaintenanceAction> {
    let disconnect_deadline = peer_timeout.saturating_mul(2);
    let mut actions = Vec::new();
    let mut survivors: Vec<(SocketAddr, &PeerInfo)> = Vec::new();

    for (addr, info) in infos {
        let silence = now.saturating_duration_since(info.last_seen);
        if info.should_disconnect() || silence > disconnect_deadline {
            actions.push(MaintenanceAction::Disconnect(*addr));
        } else {
            if silence > peer_timeout && info.state == PeerState::Connected {
                actions.push(MaintenanceAction::MarkStale(*addr));
            }
            survivors.push((*addr, info));
        }
    }

    if survivors.len() > max_peers {
        let excess = survivors.len().saturating_sub(max_peers);
        survivors.sort_by(|(_, a), (_, b)| {
            a.health_score()
                .partial_cmp(&b.health_score())
                .unwrap_or(Ordering::Equal)
        });
        actions.extend(
            survivors
                .into_iter()
                .take(excess)
                .map(|(addr, _)| MaintenanceAction::Disconnect(addr)),
        );
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PeerId;

    fn connected_peer(addr: SocketAddr) -> PeerInfo {
        let mut info = PeerInfo::new(PeerId(addr));
        info.state = PeerState::Connected;
        info
    }

    #[test]
    fn marks_silent_peer_stale() {
        let addr = "127.0.0.1:1".parse().unwrap();
        let info = connected_peer(addr);
        let now = info.last_seen + Duration::from_secs(15);

        let actions = plan_maintenance(&[(addr, info)], now, Duration::from_secs(10), 50);
        assert_eq!(actions, vec![MaintenanceAction::MarkStale(addr)]);
    }

    #[test]
    fn disconnects_after_consecutive_failures() {
        let addr = "127.0.0.1:2".parse().unwrap();
        let mut info = connected_peer(addr);
        info.consecutive_failures = 10;
        let now = info.last_seen;

        let actions = plan_maintenance(&[(addr, info)], now, Duration::from_secs(10), 50);
        assert_eq!(actions, vec![MaintenanceAction::Disconnect(addr)]);
    }

    #[test]
    fn disconnects_after_prolonged_silence() {
        let addr = "127.0.0.1:3".parse().unwrap();
        let info = connected_peer(addr);
        let now = info.last_seen + Duration::from_secs(25);

        let actions = plan_maintenance(&[(addr, info)], now, Duration::from_secs(10), 50);
        assert_eq!(actions, vec![MaintenanceAction::Disconnect(addr)]);
    }

    #[test]
    fn evicts_lowest_health_over_cap() {
        let healthy_addr = "127.0.0.1:4".parse().unwrap();
        let unhealthy_addr = "127.0.0.1:5".parse().unwrap();

        let mut healthy = connected_peer(healthy_addr);
        healthy.messages_sent = 100;
        healthy.message_failures = 0;

        let mut unhealthy = connected_peer(unhealthy_addr);
        unhealthy.messages_sent = 1;
        unhealthy.message_failures = 50;

        let now = healthy.last_seen;
        let actions = plan_maintenance(
            &[(healthy_addr, healthy), (unhealthy_addr, unhealthy)],
            now,
            Duration::from_secs(10),
            1,
        );
        assert_eq!(actions, vec![MaintenanceAction::Disconnect(unhealthy_addr)]);
    }

    #[test]
    fn fanout_excludes_sender_origin_and_mapped_connection() {
        let origin: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let origin_conn: SocketAddr = "127.0.0.1:55000".parse().unwrap();
        let sender: SocketAddr = "127.0.0.1:55001".parse().unwrap();

        let canonical: ListeningAddrs = DashMap::new();
        canonical.insert(origin, origin_conn);

        let exclude = fanout_exclusions(sender, origin, &canonical);
        assert!(
            exclude.contains(&sender),
            "the immediate sender is excluded"
        );
        assert!(
            exclude.contains(&origin),
            "the origin's canonical address is excluded"
        );
        assert!(
            exclude.contains(&origin_conn),
            "the origin's mapped connection is excluded"
        );
    }
}
