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

   /// Anti-entropy digest (message IDs this node knows about)
   AntiEntropyDigest { message_ids: Vec<MessageId> },

   /// Request for specific messages by ID
   MessageRequest { ids: Vec<MessageId> },

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

Nodes track seen messages in a `DashMap<MessageId, MessageEntry>` where `MessageEntry` contains:

- `timestamp`: When the message was first seen
- `forward_count`: How many times this node has forwarded the message
- `ttl`: Current TTL value

Messages are evicted after 5 minutes (configurable via `message_dedup_ttl`).

## TTL Mechanism

- Default TTL: 10
- Decremented on each hop
- Message not forwarded when TTL = 0
- Prevents infinite loops

## Epidemic Broadcast

1. Node creates message with TTL (default: 10)
2. Sends to `fanout` random peers (default: 3)
3. Receiving peers:
   - Check if already seen (deduplication)
   - Store in `seen_messages` cache
   - Make probabilistic forwarding decision (default: 70% probability)
   - Check forward count limit (default: max 5 forwards)
4. If forwarding:
   - Decrement TTL
   - Increment forward count
   - Re-gossip to random peers
5. Process repeats until TTL = 0 or max forwards reached

Configuration:

- `epidemic.forward_probability`: Probability of forwarding (default: 0.7)
- `epidemic.max_forwards`: Maximum forwards per node (default: 5)

## Anti-Entropy

Implemented to ensure eventual consistency:

1. Periodically (default: every 30s) exchange message digests with peers
2. Each node sends `AntiEntropyDigest` containing all known `MessageId`s
3. Receiving peer compares digest with local `seen_messages`
4. If missing messages detected:
   - Send `MessageRequest` with missing `MessageId`s
   - Peer responds with `MessageResponse` containing full messages
5. Requested messages are processed normally (deduplicated, forwarded if needed)

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
