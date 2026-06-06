//! Anti-entropy protocol for ensuring message delivery.
//!
//! Periodically exchanges message digests with peers to detect
//! and repair missing messages.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use tokio::time;
use tracing::{debug, trace, warn};

use crate::{Message, MessageId, Payload, Result, Tcp};

/// Anti-entropy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyConfig {
    /// Interval between anti-entropy rounds
    pub interval: Duration,

    /// Number of peers to sync with per round
    pub fanout: usize,

    /// Enable anti-entropy protocol
    pub enabled: bool,
}

impl Default for AntiEntropyConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            fanout: 3,
            enabled: true,
        }
    }
}

/// Represents a digest of known messages.
#[derive(Debug, Clone)]
pub struct MessageDigest {
    /// Set of known message IDs
    pub message_ids: HashSet<MessageId>,
}

impl MessageDigest {
    /// Create a new empty digest.
    pub fn new() -> Self {
        Self {
            message_ids: HashSet::new(),
        }
    }

    /// Add a message ID to the digest.
    pub fn add(&mut self, id: MessageId) {
        self.message_ids.insert(id);
    }

    /// Compute the difference between this (`self`) digest and another (`other`),
    /// returning the values that are in `self` but not `other`.
    pub fn diff(&self, other: &MessageDigest) -> Vec<MessageId> {
        other
            .message_ids
            .difference(&self.message_ids)
            .copied()
            .collect()
    }

    /// Convert to vector for serialization.
    pub fn to_vec(&self) -> Vec<MessageId> {
        self.message_ids.iter().copied().collect()
    }

    /// Create from vector.
    pub fn from_vec(ids: Vec<MessageId>) -> Self {
        Self {
            message_ids: ids.into_iter().collect(),
        }
    }
}

impl Default for MessageDigest {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry tracking a seen message.
#[derive(Debug, Clone)]
pub struct MessageEntry {
    /// The full message
    pub message: Message,
    /// When we first saw it
    pub first_seen: Instant,
    /// Number of times forwarded
    pub forward_count: u32,
}

/// Anti-entropy engine for message repair.
pub struct AntiEntropy {
    config: AntiEntropyConfig,
    transport: Arc<Tcp>,
    seen_messages: Arc<DashMap<MessageId, MessageEntry>>,
}

impl AntiEntropy {
    /// Create new anti-entropy engine.
    pub fn new(
        config: AntiEntropyConfig,
        transport: Arc<Tcp>,
        seen_messages: Arc<DashMap<MessageId, MessageEntry>>,
    ) -> Self {
        Self {
            config,
            transport,
            seen_messages,
        }
    }

    /// Start anti-entropy rounds.
    pub async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            debug!("Anti-entropy protocol disabled");
            return Ok(());
        }

        let config = self.config.clone();
        let transport = Arc::clone(&self.transport);
        let seen_messages = Arc::clone(&self.seen_messages);

        tokio::spawn(async move {
            let mut ticker = time::interval(config.interval);
            loop {
                ticker.tick().await;

                let local_addr = match transport.local_addr() {
                    Some(addr) => addr,
                    None => continue,
                };

                let mut peer_addrs = transport.peers();
                if peer_addrs.is_empty() {
                    continue;
                }

                let selected_peers: Vec<SocketAddr> = {
                    let mut rng = rand::rng();
                    peer_addrs.shuffle(&mut rng);
                    peer_addrs.into_iter().take(config.fanout).collect()
                };

                let digest = Self::build_digest(&seen_messages);
                let message_ids = digest.to_vec();

                trace!(
                    "Anti-entropy round: sending digest with {} messages to {} peers",
                    message_ids.len(),
                    selected_peers.len()
                );

                for peer_addr in selected_peers {
                    let digest_msg = Message::new(
                        local_addr,
                        Payload::AntiEntropyDigest {
                            message_ids: message_ids.clone(),
                        },
                    );

                    if let Err(e) = transport.send(peer_addr, digest_msg).await {
                        debug!("Failed to send anti-entropy digest to {peer_addr}: {e}");
                    }
                }
            }
        });

        Ok(())
    }

    fn build_digest(seen_messages: &DashMap<MessageId, MessageEntry>) -> MessageDigest {
        let mut digest = MessageDigest::new();
        for entry in seen_messages.iter() {
            digest.add(*entry.key());
        }
        digest
    }

    /// Handle incoming anti-entropy digest.
    pub async fn handle_digest(
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        remote_message_ids: Vec<MessageId>,
        transport: &Arc<Tcp>,
        seen_messages: &DashMap<MessageId, MessageEntry>,
    ) -> Result<()> {
        let remote_digest = MessageDigest::from_vec(remote_message_ids);
        let local_digest = Self::build_digest(seen_messages);

        let missing_from_remote = local_digest.diff(&remote_digest);

        if !missing_from_remote.is_empty() {
            debug!(
                "Peer {} is missing {} messages, sending them",
                peer_addr,
                missing_from_remote.len()
            );

            let mut messages_to_send = Vec::new();
            for msg_id in &missing_from_remote {
                if let Some(entry) = seen_messages.get(msg_id) {
                    messages_to_send.push(entry.value().message.clone());
                }
            }

            if !messages_to_send.is_empty() {
                let response = Message::new(
                    local_addr,
                    Payload::MessageResponse {
                        messages: messages_to_send,
                    },
                );

                if let Err(e) = transport.send(peer_addr, response).await {
                    warn!("Failed to send message response to {peer_addr}: {e}");
                }
            }
        }

        let missing_from_local = remote_digest.diff(&local_digest);

        if !missing_from_local.is_empty() {
            debug!(
                "We are missing {} messages from {}, requesting them",
                missing_from_local.len(),
                peer_addr
            );

            let request = Message::new(
                local_addr,
                Payload::MessageRequest {
                    ids: missing_from_local,
                },
            );

            if let Err(e) = transport.send(peer_addr, request).await {
                warn!("Failed to send message request to {peer_addr}: {e}");
            }
        }

        Ok(())
    }

    /// Handle message request.
    pub async fn handle_message_request(
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        requested_ids: Vec<MessageId>,
        transport: &Arc<Tcp>,
        seen_messages: &DashMap<MessageId, MessageEntry>,
    ) -> Result<()> {
        let mut messages_to_send = Vec::new();

        for msg_id in &requested_ids {
            if let Some(entry) = seen_messages.get(msg_id) {
                messages_to_send.push(entry.value().message.clone());
            }
        }

        if !messages_to_send.is_empty() {
            debug!(
                "Sending {} requested messages to {}",
                messages_to_send.len(),
                peer_addr
            );

            let response = Message::new(
                local_addr,
                Payload::MessageResponse {
                    messages: messages_to_send,
                },
            );

            if let Err(e) = transport.send(peer_addr, response).await {
                warn!("Failed to send message response to {peer_addr}: {e}");
            }
        }

        Ok(())
    }

    /// Handle message response containing missing messages.
    pub fn handle_message_response(
        messages: Vec<Message>,
        seen_messages: &DashMap<MessageId, MessageEntry>,
        message_handler: &Option<Arc<dyn Fn(SocketAddr, bytes::Bytes) + Send + Sync>>,
    ) {
        debug!(
            "Received {} missing messages via anti-entropy",
            messages.len()
        );

        for message in messages {
            if seen_messages.contains_key(&message.id) {
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

            if let Payload::Application(ref data) = message.payload
                && let Some(handler) = message_handler
            {
                handler(message.id.origin, data.clone());
            }
        }
    }
}
