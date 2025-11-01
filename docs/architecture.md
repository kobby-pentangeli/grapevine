# Grapevine Architecture

This document describes the internal architecture of Grapevine.

## Overview

Grapevine is structured in layers:

┌─────────────────────────────────┐
│         Application             │
├─────────────────────────────────┤
│      Node API (core::Node)      │
├─────────────────────────────────┤
│    Gossip Protocol Engine       │
│  (epidemic + anti-entropy)      │
├─────────────────────────────────┤
│      Transport Layer            │
│       (TCP / QUIC)              │
├─────────────────────────────────┤
│      Message Codec              │
└─────────────────────────────────┘

## Components

### Core Types (`src/core/`)

- **Message**: Gossip message structure with ID, TTL, and payload
- **Peer**: Represents a connected peer
- **Node**: High-level API for applications

### Protocol (`src/protocol/`)

- **Gossip**: Main protocol engine
- **Epidemic**: Probabilistic broadcast
- **Anti-Entropy**: Periodic sync

### Transport (`src/transport/`)

- **TcpTransport**: TCP-based transport
- **QuicTransport**: QUIC-based transport (optional)

### Codec (`src/codec/`)

- **MessageCodec**: Length-prefixed framing and serialization

## Message Flow

1. Application calls `node.broadcast(data)`
2. Protocol creates `Message` with unique ID
3. Message is serialized via `MessageCodec`
4. Transport sends to random subset of peers (fan-out)
5. Receiving nodes:
   - Deserialize message
   - Check if already seen (deduplication)
   - Forward to application handler
   - Re-gossip to other peers (if TTL > 0)

## Peer Discovery

1. Node connects to bootstrap peers
2. Requests peer list via `PeerListRequest`
3. Receives `PeerListResponse` with known peers
4. Connects to discovered peers
5. Repeats until reaching `max_peers`

## Heartbeat & Failure Detection

- Nodes send periodic heartbeats
- `last_seen` timestamp updated on any message
- Peers not seen within `peer_timeout` are removed
- Peer maintenance runs every 10 seconds

## Configuration

All behavior is configurable via `NodeConfig`:

- `gossip_interval`: How often to gossip
- `fanout`: Number of peers per gossip round
- `max_peers`: Maximum peer connections
- `peer_timeout`: Stale peer timeout

See `NodeConfig` documentation for all options.
