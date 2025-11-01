# Gossip Protocol Specification

## Message Types

### Application Message

```rust
MessagePayload::Application(Bytes)
```

User data to be disseminated.

### Peer List Request

```rust
MessagePayload::PeerListRequest
```

Request for a peer's known peers.

### Peer List Response

```rust
MessagePayload::PeerListResponse { peers: Vec<SocketAddr> }
```

Response containing known peers.

### Heartbeat

```rust
MessagePayload::Heartbeat { from: SocketAddr }
```

Keep-alive message.

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

Nodes track seen IDs in a `DashMap<MessageId, ()>`.

## TTL Mechanism

- Default TTL: 10
- Decremented on each hop
- Message not forwarded when TTL = 0
- Prevents infinite loops

## Epidemic Broadcast

1. Node creates message with TTL
2. Sends to `fanout` random peers
3. Receiving peers decrement TTL
4. If TTL > 0, re-gossip to their random peers
5. Process repeats until TTL = 0

## Anti-Entropy (Future Work)

Planned for v1.1:

1. Periodically exchange message digests with peers
2. Detect missing messages
3. Request missing messages explicitly
4. Ensures eventual consistency
