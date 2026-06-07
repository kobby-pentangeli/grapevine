//! Anti-entropy protocol for ensuring message delivery.
//!
//! Periodically reconciles the broadcast set with peers: each node
//! summarizes what it holds as a per-origin `origin -> next-needed sequence` map and
//! exchanges deltas, instead of shipping the full set of known message identifiers every round.

use std::collections::{BTreeSet, HashMap};
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

/// Entry tracking a seen message.
#[derive(Debug, Clone)]
pub struct MessageEntry {
    /// The full message
    pub message: Message,
    /// When we first saw it
    pub first_seen: Instant,
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

                let version_vector = build_version_vector(&seen_messages);

                trace!(
                    "Anti-entropy round: sending version vector ({} origins) to {} peers",
                    version_vector.len(),
                    selected_peers.len()
                );

                for peer_addr in selected_peers {
                    let digest_msg = Message::new(
                        local_addr,
                        0,
                        Payload::AntiEntropyDigest {
                            version_vector: version_vector.clone(),
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

    /// Handle an incoming anti-entropy digest (the peer's version vector).
    pub async fn handle_digest(
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        remote_version_vec: Vec<(SocketAddr, u64)>,
        transport: &Arc<Tcp>,
        seen_messages: &DashMap<MessageId, MessageEntry>,
    ) -> Result<()> {
        let remote = remote_version_vec
            .into_iter()
            .collect::<HashMap<SocketAddr, u64>>();

        let to_send = messages_for_peer(seen_messages, &remote);
        if !to_send.is_empty() {
            debug!("Pushing {} messages to {}", to_send.len(), peer_addr);
            Self::send_message_responses(local_addr, peer_addr, to_send, transport).await;
        }

        let request = Message::new(
            local_addr,
            0,
            Payload::MessageRequest {
                version_vector: build_version_vector(seen_messages),
            },
        );
        if let Err(e) = transport.send(peer_addr, request).await {
            warn!("Failed to send message request to {peer_addr}: {e}");
        }

        Ok(())
    }

    /// Handle a pull request: push every message we hold beyond the peer's
    /// version vector. Terminal in the exchange (no further request is sent).
    pub async fn handle_message_request(
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        remote_version_vec: Vec<(SocketAddr, u64)>,
        transport: &Arc<Tcp>,
        seen_messages: &DashMap<MessageId, MessageEntry>,
    ) -> Result<()> {
        let remote: HashMap<SocketAddr, u64> = remote_version_vec.into_iter().collect();

        let to_send = messages_for_peer(seen_messages, &remote);
        if !to_send.is_empty() {
            debug!(
                "Pushing {} requested messages to {}",
                to_send.len(),
                peer_addr
            );
            Self::send_message_responses(local_addr, peer_addr, to_send, transport).await;
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

/// Summarize the broadcast set as a per-origin version vector: for each origin,
/// the lowest sequence not yet held (the length of the contiguous prefix from
/// `0`). A peer pushes back everything it holds at or above this sequence.
fn build_version_vector(
    seen_messages: &DashMap<MessageId, MessageEntry>,
) -> Vec<(SocketAddr, u64)> {
    let mut sequences: HashMap<SocketAddr, BTreeSet<u64>> = HashMap::new();
    for entry in seen_messages.iter() {
        let id = entry.key();
        sequences.entry(id.origin).or_default().insert(id.sequence);
    }

    sequences
        .into_iter()
        .map(|(origin, seqs)| (origin, next_needed_sequence(&seqs)))
        .collect()
}

/// The first sequence missing from `0`: the length of the contiguous prefix.
/// Since `sequences` is sorted and unique, the prefix holds exactly while the
/// `i`-th smallest element equals `i`.
fn next_needed_sequence(sequences: &BTreeSet<u64>) -> u64 {
    sequences
        .iter()
        .enumerate()
        .take_while(|&(index, &sequence)| u64::try_from(index).is_ok_and(|i| i == sequence))
        .count()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// Every message held at or above the peer's per-origin need (default `0` for an
/// origin the peer has never seen). Conservative under gaps: a peer that holds
/// messages past its own gap may receive a few it already has, which it dedups.
fn messages_for_peer(
    seen_messages: &DashMap<MessageId, MessageEntry>,
    remote: &HashMap<SocketAddr, u64>,
) -> Vec<Message> {
    seen_messages
        .iter()
        .filter(|entry| {
            let id = entry.key();
            id.sequence >= remote.get(&id.origin).copied().unwrap_or(0)
        })
        .map(|entry| entry.value().message.clone())
        .collect()
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
        0,
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
        |messages: Vec<Message>| Message::new(local_addr, 0, Payload::MessageResponse { messages });

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

    fn broadcast(origin: SocketAddr, sequence: u64) -> Message {
        Message::new(origin, sequence, Payload::Application(vec![0u8; 8].into()))
    }

    fn seen(messages: impl IntoIterator<Item = Message>) -> DashMap<MessageId, MessageEntry> {
        let map = DashMap::new();
        for message in messages {
            map.insert(
                message.id,
                MessageEntry {
                    message,
                    first_seen: Instant::now(),
                },
            );
        }
        map
    }

    fn app_message(origin: SocketAddr, byte: u8, len: usize) -> Message {
        Message::new(
            origin,
            u64::from(byte),
            Payload::Application(vec![byte; len].into()),
        )
    }

    fn frame_len(message: &Message) -> usize {
        bincode::serde::encode_to_vec(message, bincode::config::standard())
            .expect("message encodes")
            .len()
    }

    #[test]
    fn version_vector_reports_next_needed_per_origin() {
        let a = addr(10);
        let b = addr(11);
        // a holds {0,1,2} -> needs 3; b holds {0,2} (gap at 1) -> needs 1.
        let map = seen([
            broadcast(a, 0),
            broadcast(a, 1),
            broadcast(a, 2),
            broadcast(b, 0),
            broadcast(b, 2),
        ]);

        let vv = build_version_vector(&map)
            .into_iter()
            .collect::<HashMap<SocketAddr, u64>>();
        assert_eq!(
            vv.get(&a),
            Some(&3),
            "contiguous prefix advances to the gap"
        );
        assert_eq!(vv.get(&b), Some(&1), "a gap caps the prefix at its start");
    }

    #[test]
    fn version_vector_with_a_leading_gap_needs_zero() {
        let a = addr(10);
        // Missing sequence 0 entirely: nothing is contiguous, so we need 0.
        let map = seen([broadcast(a, 1), broadcast(a, 2)]);
        let vv = build_version_vector(&map)
            .into_iter()
            .collect::<HashMap<SocketAddr, u64>>();
        assert_eq!(vv.get(&a), Some(&0));
    }

    #[test]
    fn messages_for_peer_sends_at_or_above_remote_need() {
        let a = addr(10);
        let map = seen([broadcast(a, 0), broadcast(a, 1), broadcast(a, 2)]);

        let remote = HashMap::from([(a, 1u64)]);
        let mut seqs = messages_for_peer(&map, &remote)
            .iter()
            .map(|m| m.id.sequence)
            .collect::<Vec<u64>>();
        seqs.sort_unstable();
        assert_eq!(seqs, vec![1, 2], "only sequences >= the need are pushed");

        let all = messages_for_peer(&map, &HashMap::new());
        assert_eq!(
            all.len(),
            3,
            "an unknown origin defaults to needing everything"
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
