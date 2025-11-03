//! Core gossip protocol engine.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use rand::seq::SliceRandom;
use tokio::sync::{RwLock, broadcast};
use tokio::time::{self, sleep};
use tracing::{debug, info, trace, warn};

use crate::transport::tcp::TcpTransport;
use crate::{Error, Message, MessageId, NodeConfig, Payload, Peer, PeerId, Result};

/// Main gossip protocol engine.
pub struct Gossip {
    /// Node configuration
    config: NodeConfig,

    /// TCP transport
    transport: Arc<RwLock<TcpTransport>>,

    /// Connected peers
    peers: Arc<DashMap<PeerId, Peer>>,

    /// Seen message IDs with timestamps (for deduplication with time-based expiry)
    seen_messages: Arc<DashMap<MessageId, Instant>>,

    /// Application message handler
    message_handler: Option<Arc<dyn Fn(SocketAddr, Bytes) + Send + Sync>>,

    /// Shutdown signal broadcaster
    shutdown_tx: broadcast::Sender<()>,
}

impl Gossip {
    /// Create a new gossip protocol instance.
    pub fn new(config: NodeConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            config,
            transport: Arc::new(RwLock::new(TcpTransport::new())),
            peers: Arc::new(DashMap::new()),
            seen_messages: Arc::new(DashMap::new()),
            message_handler: None,
            shutdown_tx,
        }
    }

    /// Set the application message handler.
    pub fn set_message_handler<F>(&mut self, handler: F)
    where
        F: Fn(SocketAddr, Bytes) + Send + Sync + 'static,
    {
        self.message_handler = Some(Arc::new(handler));
    }

    /// Start the gossip protocol.
    pub async fn start(&mut self) -> Result<()> {
        {
            let mut transport = self.transport.write().await;
            transport.listen(self.config.bind_addr).await?;
            let local_addr = transport
                .local_addr()
                .ok_or_else(|| Error::internal("Transport has no local address after listening"))?;
            info!("Gossip node started on {local_addr}");
        }

        for peer in &self.config.bootstrap_peers {
            if let Err(e) = self.connect_to_peer(*peer).await {
                warn!("Failed to connect to bootstrap peer {peer}: {e}");
            }
        }

        self.spawn_message_receiver();
        self.spawn_gossip_loop();
        self.spawn_peer_maintenance();
        self.spawn_message_cleanup();

        Ok(())
    }

    /// Connect to a peer.
    pub async fn connect_to_peer(&self, addr: SocketAddr) -> Result<()> {
        let transport = self.transport.read().await;
        transport.connect(addr).await?;

        let local_addr = transport
            .local_addr()
            .ok_or_else(|| Error::internal("No local address"))?;
        let message = Message::new(local_addr, Payload::PeerListRequest);
        transport.send(addr, message).await?;

        info!("Connected to peer {addr}");
        Ok(())
    }

    /// Broadcast a message to the network.
    pub async fn broadcast(&self, data: Bytes) -> Result<()> {
        let local_addr = self
            .transport
            .read()
            .await
            .local_addr()
            .ok_or_else(|| Error::internal("No local address"))?;

        let message = Message::new(local_addr, Payload::Application(data));

        // Mark as seen with current timestamp
        self.seen_messages.insert(message.id, Instant::now());

        self.gossip_message(message).await
    }

    /// Get local address.
    pub async fn local_addr(&self) -> Option<SocketAddr> {
        self.transport.read().await.local_addr()
    }

    /// Get list of connected peers.
    pub async fn peer_list(&self) -> Vec<SocketAddr> {
        self.transport.read().await.peers()
    }

    /// Shutdown the node.
    pub async fn shutdown(&self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        sleep(Duration::from_millis(100)).await;
        Ok(())
    }

    fn spawn_message_receiver(&self) {
        let transport = Arc::clone(&self.transport);
        let peers = Arc::clone(&self.peers);
        let seen_messages = Arc::clone(&self.seen_messages);
        let message_handler = self.message_handler.clone();
        let config = self.config.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            loop {
                let recv_fut = async { transport.read().await.recv().await };

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

                trace!("Received message from {peer_addr}: {:?}", message.id);

                // Handle protocol messages
                match &message.payload {
                    Payload::PeerListRequest => {
                        Self::handle_peer_list_request(&transport, peer_addr).await;
                    }
                    Payload::PeerListResponse { peers: peer_list } => {
                        Self::handle_peer_list_response(&transport, peer_list).await;
                    }
                    Payload::Application(data) => {
                        if seen_messages.contains_key(&message.id) {
                            trace!("Duplicate message {}, ignoring", message.id);
                            continue;
                        }
                        seen_messages.insert(message.id, Instant::now());
                        if let Some(ref handler) = message_handler {
                            handler(message.id.origin, data.clone());
                        }

                        if message.ttl > 1 {
                            let mut new_message = message.clone();
                            new_message.decrement_ttl();
                            let _ = Self::gossip_to_random_peers(
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
                        let transport_guard = transport.read().await;
                        let local_addr = match transport_guard.local_addr() {
                            Some(addr) => addr,
                            None => continue,
                        };

                        let heartbeat = Message::new(local_addr, Payload::Heartbeat { from: local_addr });
                        let peer_addrs = transport_guard.peers();

                        for peer_addr in peer_addrs {
                            if let Err(e) = transport_guard.send(peer_addr, heartbeat.clone()).await {
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
            let mut ticker = time::interval(Duration::from_secs(10));
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        debug!("Peer maintenance shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        let stale_peers = peers
                            .iter()
                            .filter(|entry| entry.value().info.is_stale(timeout))
                            .map(|entry| *entry.key())
                            .collect::<Vec<PeerId>>();

                        for peer_id in stale_peers {
                            info!("Removing stale peer {peer_id}");
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
            let mut ticker = time::interval(Duration::from_secs(30));
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
                            .filter(|entry| now.duration_since(*entry.value()) > ttl)
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
        Self::gossip_to_random_peers(&self.transport, &self.peers, message, self.config.fanout)
            .await
    }

    async fn gossip_to_random_peers(
        transport: &Arc<RwLock<TcpTransport>>,
        _peers: &Arc<DashMap<PeerId, Peer>>,
        message: Message,
        fanout: usize,
    ) -> Result<()> {
        let transport_guard = transport.read().await;
        let mut peer_addrs = transport_guard.peers();

        if peer_addrs.is_empty() {
            return Ok(());
        }

        let selected: Vec<_> = {
            let mut rng = rand::thread_rng();
            peer_addrs.shuffle(&mut rng);
            peer_addrs.into_iter().take(fanout).collect()
        };

        for addr in selected {
            if let Err(e) = transport_guard.send(addr, message.clone()).await {
                warn!("Failed to gossip to {addr}: {e}");
            }
        }

        Ok(())
    }

    async fn handle_peer_list_request(transport: &Arc<RwLock<TcpTransport>>, sender: SocketAddr) {
        let transport_guard = transport.read().await;
        let peer_list = transport_guard.peers();

        let local_addr = match transport_guard.local_addr() {
            Some(addr) => addr,
            None => return,
        };

        let response = Message::new(local_addr, Payload::PeerListResponse { peers: peer_list });
        if let Err(e) = transport_guard.send(sender, response).await {
            warn!("Failed to send peer list to {sender}: {e}");
        }
    }

    async fn handle_peer_list_response(
        transport: &Arc<RwLock<TcpTransport>>,
        peer_list: &[SocketAddr],
    ) {
        for &peer_addr in peer_list {
            if let Err(e) = transport.read().await.connect(peer_addr).await {
                debug!("Failed to connect to peer {peer_addr}: {e}");
            }
        }
    }
}
