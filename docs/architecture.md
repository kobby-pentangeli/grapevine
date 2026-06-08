# Grapevine Architecture

This document describes the internal architecture of Grapevine.

## Overview

Grapevine is structured in layers:

┌─────────────────────────────────┐
│          Application            │
├─────────────────────────────────┤
│           Node API              │
├─────────────────────────────────┤
│         Protocol Engine         │
│(gossip, epidemic, anti-entropy) │
├─────────────────────────────────┤
│        Transport Layer          │
│       (TCP, QUIC, etc)          │
├─────────────────────────────────┤
│          Core Types             │
└─────────────────────────────────┘

## Components

### Core Types (`src/core/`)

- **Message**: Gossip message structure with ID, TTL, payload, and the origin's public key plus signature
- **MessageCodec**: Length-prefixed framing and bincode serialization. Message size limit: 10MB default (configurable)
- **Identity**: Per-node Ed25519 keypair; signs authored messages and exposes the node's `PeerId`
- **PeerId**: Cryptographic node identity (the Ed25519 public key)
- **Peer**: Represents a connected peer with health tracking
- **PeerInfo**: Per-connection metadata with health score, failure tracking, state machine
- **RateLimiter**: Per-peer token bucket rate limiting (100 capacity, 50 tokens/sec)

### Transport Layer (`src/transport/`)

- **Tcp**: TCP-based transport that owns the authoritative peer registry
  - Each outbound message is encoded exactly once, by the peer's writer task at the socket
  - Per-peer write channels are bounded and lossy under backpressure (drop-newest); the shared inbound channel is bounded and applies backpressure to readers
  - Shutdown stops accepting, flushes queued frames (such as goodbyes), and awaits every connection task instead of sleeping a fixed grace period
- Note: QUIC transport planned for a later release

### Protocol Engine (`src/protocol/`)

- **Gossip**: Main protocol engine with background tasks
- **Epidemic**: Probabilistic broadcast (70% forward probability, blind variant)
- **Anti-Entropy**: Periodic version-vector reconciliation and repair (every 30s)

## Message Flow

1. Application calls `node.broadcast(data)`
2. Protocol authors a `Message` with a per-origin `(origin, sequence)` ID and signs it with the node's `Identity`
3. Message stored in `seen_messages` cache with metadata
4. Transport enqueues the message to a random subset of peers (fan-out)
5. Each peer's writer task encodes the message once (`MessageCodec`) and writes it to the socket
6. Receiving nodes:
   - Rate limiting check (token bucket per peer)
   - Deserialize message via `MessageCodec`
   - Authenticate: verify the origin's signature and enforce the trust-on-first-use origin/key binding (reject on failure)
   - Check if already seen (deduplication via `MessageId`)
   - Store in `seen_messages` with `MessageEntry` metadata
   - Forward to application handler (if `Application` payload)
   - With probability `forward_probability` (default 70%), re-gossip once (unchanged signature) to a fanout that excludes the sender and origin (if TTL > 1)

## Peer Discovery

1. Node connects to bootstrap peers
2. Requests peer list via `PeerListRequest`
3. Receives `PeerListResponse` with known peers
4. Connects to discovered peers
5. Repeats until reaching `max_peers`

## Heartbeat & Peer Health

- Nodes send periodic heartbeats (configurable interval)
- `last_seen` timestamp updated on any message
- Peer state machine: Connecting => Connected => Stale => Disconnected
- Health score based on:
  - Success/failure ratio
  - Connection age (older connections get bonus)
  - Consecutive failures (penalty)
- Peers with 5 consecutive failures are disconnected
- Peer maintenance runs every 10 seconds
- Stale peers marked after `peer_timeout` (default: 30s)

## Configuration

All behavior is configurable via `NodeConfig`:

- `gossip_interval`: How often to send heartbeats (default: 5s)
- `fanout`: Number of peers per gossip round (default: 3)
- `max_peers`: Maximum peer connections (default: 50)
- `peer_timeout`: Stale peer timeout (default: 30s)
- `message_dedup_ttl`: How long to remember seen messages (default: 5 minutes)
- `anti_entropy`: Anti-entropy protocol configuration
  - `enabled`: Enable/disable anti-entropy (default: true)
  - `interval`: How often to sync (default: 30s)
  - `fanout`: Peers to sync with (default: 3)
- `epidemic`: Epidemic broadcast configuration
  - `forward_probability`: Probability of forwarding a newly learned rumor (default: 0.7)
- `rate_limit`: Rate limiting configuration
  - `enabled`: Enable/disable rate limiting (default: true)
  - `capacity`: Token bucket capacity (default: 100)
  - `refill_rate`: Tokens per second (default: 50)

See `NodeConfig` and `NodeConfigBuilder` documentation for all options and validation rules.
