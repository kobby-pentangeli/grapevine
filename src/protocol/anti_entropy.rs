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

/// Space, in bytes, withheld from the frame budget so that bincode's
/// variable-length count prefix can grow as a chunk fills without pushing the
/// frame past the limit.
const CHUNK_LENGTH_PREFIX_RESERVE: usize = 8;

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
        self.message_ids
            .difference(&other.message_ids)
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

            let messages_to_send = missing_from_remote
                .iter()
                .filter_map(|msg_id| seen_messages.get(msg_id).map(|e| e.value().message.clone()))
                .collect();

            Self::send_message_responses(local_addr, peer_addr, messages_to_send, transport).await;
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
        let messages_to_send = requested_ids
            .iter()
            .filter_map(|msg_id| seen_messages.get(msg_id).map(|e| e.value().message.clone()))
            .collect::<Vec<Message>>();

        if !messages_to_send.is_empty() {
            debug!(
                "Sending {} requested messages to {}",
                messages_to_send.len(),
                peer_addr
            );
            Self::send_message_responses(local_addr, peer_addr, messages_to_send, transport).await;
        }

        Ok(())
    }

    /// Send repaired `messages` as one or more frame-bounded `MessageResponse`s.
    async fn send_message_responses(
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        messages: Vec<Message>,
        transport: &Arc<Tcp>,
    ) {
        for response in chunk_message_responses(local_addr, messages, transport.max_message_size())
        {
            if let Err(e) = transport.send(peer_addr, response).await {
                warn!("Failed to send message response to {peer_addr}: {e}");
            }
        }
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

/// Split repaired `messages` into one or more `MessageResponse`s, each of which
/// serializes within `max_frame_size`.
fn chunk_message_responses(
    local_addr: SocketAddr,
    messages: Vec<Message>,
    max_frame_size: usize,
) -> Vec<Message> {
    let encoded_len = |message: &Message| {
        bincode::serde::encode_to_vec(message, bincode::config::standard())
            .ok()
            .map(|bytes| bytes.len())
    };

    let envelope = Message::new(
        local_addr,
        Payload::MessageResponse {
            messages: Vec::new(),
        },
    );
    let Some(envelope_len) = encoded_len(&envelope) else {
        return Vec::new();
    };
    let budget = max_frame_size
        .saturating_sub(envelope_len)
        .saturating_sub(CHUNK_LENGTH_PREFIX_RESERVE);

    let wrap =
        |messages: Vec<Message>| Message::new(local_addr, Payload::MessageResponse { messages });

    let mut responses = Vec::new();
    let mut batch: Vec<Message> = Vec::new();
    let mut batch_len = 0usize;

    for message in messages {
        let Some(len) = encoded_len(&message) else {
            continue;
        };
        if len > budget {
            warn!(
                "Dropping un-chunkable repair message {} ({len} B over frame budget {budget} B)",
                message.id
            );
            continue;
        }
        if !batch.is_empty() && batch_len.saturating_add(len) > budget {
            responses.push(wrap(std::mem::take(&mut batch)));
            batch_len = 0;
        }
        batch_len = batch_len.saturating_add(len);
        batch.push(message);
    }

    if !batch.is_empty() {
        responses.push(wrap(batch));
    }

    responses
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::message_codec::MAX_FRAME_SIZE;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn app_message(origin: SocketAddr, byte: u8, len: usize) -> Message {
        Message::new(origin, Payload::Application(vec![byte; len].into()))
    }

    fn frame_len(message: &Message) -> usize {
        bincode::serde::encode_to_vec(message, bincode::config::standard())
            .expect("message encodes")
            .len()
    }

    #[test]
    fn diff_returns_values_in_self_not_other() {
        let local = addr(1);
        let shared = MessageId::new(local);
        let only_self = MessageId::new(local);
        let only_other = MessageId::new(local);

        let mut a = MessageDigest::new();
        a.add(shared);
        a.add(only_self);

        let mut b = MessageDigest::new();
        b.add(shared);
        b.add(only_other);

        let a_not_b = a.diff(&b);
        assert_eq!(
            a_not_b,
            vec![only_self],
            "self.diff(other) is self minus other"
        );

        let b_not_a = b.diff(&a);
        assert_eq!(b_not_a, vec![only_other], "diff is not symmetric");

        assert!(
            a.diff(&a).is_empty(),
            "a digest differs from itself by nothing"
        );
    }

    #[test]
    fn chunking_keeps_every_frame_within_the_limit() {
        let local = addr(1);
        let origin = addr(2);
        // Each ~512 B message; a 2 KB budget forces several chunks.
        let messages = (0..16)
            .map(|i| app_message(origin, i, 512))
            .collect::<Vec<Message>>();
        let total = messages.len();
        let max_frame_size = 2048;

        let responses = chunk_message_responses(local, messages, max_frame_size);

        assert!(responses.len() > 1, "an over-budget batch must split");
        let mut carried = 0;
        for response in &responses {
            assert!(
                frame_len(response) <= max_frame_size,
                "no chunk may exceed the frame limit"
            );
            match &response.payload {
                Payload::MessageResponse { messages } => carried += messages.len(),
                other => panic!("expected MessageResponse, got {other:?}"),
            }
        }
        assert_eq!(carried, total, "chunking preserves every message");
    }

    #[test]
    fn chunking_drops_a_message_that_cannot_fit_alone() {
        let local = addr(1);
        let origin = addr(2);
        let small = app_message(origin, 1, 100);
        let oversized = app_message(origin, 2, 4096);

        let responses = chunk_message_responses(local, vec![small.clone(), oversized], 2048);

        let carried = responses
            .iter()
            .flat_map(|response| match &response.payload {
                Payload::MessageResponse { messages } => messages.iter().map(|m| m.id),
                _ => unreachable!(),
            })
            .collect::<Vec<MessageId>>();
        assert_eq!(
            carried,
            vec![small.id],
            "only the deliverable message survives"
        );
    }

    #[test]
    fn chunking_fits_a_small_batch_in_one_frame() {
        let local = addr(1);
        let origin = addr(2);
        let messages = vec![app_message(origin, 1, 64), app_message(origin, 2, 64)];

        let responses = chunk_message_responses(local, messages, MAX_FRAME_SIZE);
        assert_eq!(responses.len(), 1, "a small batch needs a single frame");
    }
}
