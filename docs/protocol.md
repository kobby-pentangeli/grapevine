# Grapevine Protocol Specification

## Overview

Grapevine implements a **push-based epidemic gossip protocol** for reliable message dissemination in distributed peer-to-peer networks. The protocol combines two complementary mechanisms:

1. **Epidemic Broadcast** - Fast, probabilistic message propagation
2. **Anti-Entropy** - Periodic synchronization for eventual consistency

This hybrid approach provides:

- **High throughput** - Messages spread rapidly through the network
- **Reliability** - Anti-entropy ensures no messages are permanently lost
- **Scalability** - Probabilistic forwarding prevents message storms
- **Fault tolerance** - No single point of failure, tolerates node failures

## Protocol Guarantees

### Eventual Consistency

All nodes eventually receive all messages, even in the presence of:

- Message loss due to probabilistic forwarding
- Temporary network partitions
- Node failures and restarts
- Race conditions in message arrival

### Deduplication

Messages are delivered exactly once per node through:

- Unique message identifiers (origin + sequence + timestamp)
- Per-message deduplication cache with configurable TTL
- Forward count tracking to prevent re-forwarding loops

### Bounded Message Propagation

Messages do not propagate indefinitely through:

- TTL mechanism (default: 10 hops)
- Per-node forward count limits (default: max 5 forwards)
- Time-based message eviction (default: 5 minutes)

### DoS Protection

The protocol resists denial-of-service attacks via:

- Per-peer token bucket rate limiting (100 capacity, 50 tokens/sec)
- Maximum message size limits (default: 10MB)
- Automatic peer health tracking and disconnection

## Protocol Phases

### 1. Bootstrap Phase

When a node starts:

1. Connects to configured bootstrap peers
2. Sends `PeerListRequest` to each bootstrap peer
3. Receives `PeerListResponse` with known peer addresses
4. Connects to discovered peers until reaching `max_peers` limit
5. Begins participating in gossip once connected to at least one peer

### 2. Active Gossip Phase

During normal operation:

1. **Message Broadcast**: Application broadcasts messages via epidemic protocol
2. **Heartbeats**: Nodes send periodic heartbeats to maintain peer health
3. **Anti-Entropy**: Periodic digest exchange ensures consistency
4. **Peer Maintenance**: Automatic health monitoring and peer replacement

### 3. Shutdown Phase

When a node shuts down gracefully:

1. Sends `Goodbye` messages to all connected peers
2. Stops accepting new messages
3. Waits for in-flight messages to complete (timeout: 500ms)
4. Closes all peer connections
5. Cleans up resources

## Message Types (Payload Variants)

```rust
   /// User-defined application data
   Application(Bytes),

   /// Peer discovery request
   PeerDiscovery,

   /// Peer list announcement
   PeerAnnouncement { peers: Vec<SocketAddr> },

   /// Heartbeat/keep-alive
   Heartbeat { from: SocketAddr },

   /// Request for peer list
   PeerListRequest,

   /// Response with peer list
   PeerListResponse { peers: Vec<SocketAddr> },

   /// Anti-entropy digest: the sender's per-origin version vector
   AntiEntropyDigest { version_vector: Vec<(SocketAddr, u64)> },

   /// Pull request: the sender's per-origin version vector
   MessageRequest { version_vector: Vec<(SocketAddr, u64)> },

   /// Response containing requested messages
   MessageResponse { messages: Vec<Message> },

   /// Graceful shutdown notification
   Goodbye { reason: String },

   /// Direct message to a specific peer (not gossiped)
   DirectMessage { recipient: SocketAddr, data: Bytes },
```

## Wire Format

Messages use length-prefixed framing:

```txt
┌────────────┬──────────────────────┐
│ Length (4) │  Bincode Payload     │
│   bytes    │    (Length bytes)    │
└────────────┴──────────────────────┘
```

Length is big-endian `u32`.

## Message Deduplication

Each message has a unique ID:

```rust
struct MessageId {
    origin: SocketAddr,
    sequence: u64,
    timestamp: u64,
}
```

Identity is `(origin, sequence)`; `timestamp` is metadata and is excluded from equality and hashing, so a node's wall clock cannot affect message identity. Only broadcast (`Application`) messages are stored and reconciled here. Direct messages are unicast and single-hop, and control messages are handled on receipt, so neither is deduplicated through this cache.

Nodes track seen messages in a `DashMap<MessageId, MessageEntry>` where `MessageEntry` contains:

- `message`: The full message
- `first_seen`: When the message was first seen (drives dedup-cache eviction)

Messages are evicted after 5 minutes (configurable via `message_dedup_ttl`).

## TTL Mechanism

- Default TTL: 10
- Decremented on each hop
- Message not forwarded when TTL = 0
- Prevents infinite loops

## Epidemic Broadcast

1. Node creates a message with a TTL (default: 10) and its next per-origin sequence
2. Sends to `fanout` random peers (default: 3)
3. Receiving peers:
   - Check if already seen (deduplication)
   - Store in `seen_messages` cache
   - With probability `forward_probability` (default: 70%), forward once
4. If forwarding:
   - Decrement TTL
   - Re-gossip to `fanout` peers, excluding the sender and the origin so the rumor is never echoed straight back
5. Propagation stops when TTL reaches 1 or the forward coin fails; deduplication prevents any node from forwarding the same message twice

This is the "blind" rumor-mongering variant (Demers et al. 1987 §1.3). The feedback and counter variants (stop once already-infected peers are met) are a deferred optimization.

Configuration:

- `epidemic.forward_probability`: Probability of forwarding a newly learned rumor (default: 0.7)

## Anti-Entropy

Reconciles the broadcast set with peers to guarantee eventual consistency, using scuttlebutt-style version vectors (van Renesse et al. 2008 §2):

1. Periodically (default: every 30s) pick `fanout` peers to reconcile with
2. Each node sends `AntiEntropyDigest` carrying its per-origin version vector (`origin -> lowest sequence still needed`)
3. The receiving peer pushes back every message it holds at or above each advertised sequence (a `MessageResponse`, chunked to stay within the frame limit), then replies with its own version vector as a `MessageRequest`
4. The original sender answers that request the same way, completing a bounded push-pull round (Demers §1.2)
5. Repaired messages enter `seen_messages` and are re-advertised on the next round

Configuration:

- `anti_entropy.enabled`: Enable/disable anti-entropy (default: true)
- `anti_entropy.interval`: Sync interval (default: 30s)
- `anti_entropy.fanout`: Number of peers to sync with per round (default: 3)

This mechanism ensures that even if epidemic broadcast misses some nodes due to probabilistic forwarding, all nodes eventually receive all messages.

## Peer Health and Lifecycle

### Peer State Machine

Each peer connection transitions through states:

```txt
Connecting → Connected → Stale → Disconnected
     ↓            ↓         ↓
     └────────────┴─────────┴─→ (reconnect or replace)
```

- **Connecting**: Initial connection establishment
- **Connected**: Active, responding peer
- **Stale**: No messages received within `peer_timeout` (default: 30s)
- **Disconnected**: Connection lost or peer failed

### Health Scoring

Peer health is calculated from:

- **Success/failure ratio** - Recent message delivery success rate
- **Connection age** - Longer connections receive bonus score
- **Consecutive failures** - Penalty increases exponentially

Peers with 5 consecutive failures are automatically disconnected.

### Peer Maintenance

Background task runs every 10 seconds:

1. Check `last_seen` timestamp for all peers
2. Mark stale peers (no activity for `peer_timeout`)
3. Disconnect unhealthy peers (health score below threshold)
4. Attempt reconnection or discover new peers if below `max_peers`

### Peer Selection

When selecting peers for gossip:

- Random selection from connected peers (uniform distribution)
- No preference for high-health peers (ensures network coverage)
- Fanout ensures message reaches diverse subset of network

## Security Considerations

### Rate Limiting

Per-peer token bucket algorithm:

- **Capacity**: 100 tokens (burst allowance)
- **Refill rate**: 50 tokens/second (sustained rate)
- **Cost**: 1 token per message
- Peers exceeding rate are throttled (not disconnected)

Rate limiting is configurable via `rate_limit` config:

```rust
RateLimitConfig {
    enabled: true,
    capacity: 100,
    refill_rate: 50,
}
```

### Message Size Limits

Configurable maximum message size (default: 10MB):

- Prevents memory exhaustion attacks
- Enforced at codec layer before deserialization
- Messages exceeding limit are dropped with error log
