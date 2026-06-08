# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Draft notes for the upcoming `1.1.0` correctness-and-hardening release. This section is finalized (renamed, dated, with verified test counts) before the version bump.

### Added

- `Tcp::set_max_peers`, `Tcp::peer_infos`, `Tcp::mark_stale`, and `Tcp::disconnect`, exposing the transport's authoritative peer registry so membership policy is driven from a single source of truth.
- `Tcp::max_message_size`, exposing the negotiated frame limit so anti-entropy repair can chunk its responses to stay within it.
- `Tcp::shutdown`, which stops the listener and tears down every connection task deterministically.
- Cryptographic message authenticity (always on): an `Identity` type holding a per-node Ed25519 keypair, a `Signature` type, and the free functions `verify_message` and `authenticate`. Each node signs every message it originates over a domain-separated encoding of `(origin, sequence, payload)`; recipients verify and pin each origin address to the key it first presented (trust-on-first-use). See `docs/protocol.md` and the `core::identity` module docs for the threat model.
- `Node::peer_id` and `Gossip::peer_id`, exposing the node's cryptographic identity (its Ed25519 public key).
- `Error::OriginKeyMismatch`, returned when a message claims an origin already pinned to a different key.

### Changed

- The TCP transport now owns the single authoritative peer registry; the gossip engine's never-populated second peer map was removed. Every received frame refreshes the sender's liveness and drives the `Connecting -> Connected -> Stale -> Disconnected` state machine, so staleness, health scoring, and peer maintenance operate on real data instead of an empty map.
- Peer-list discovery advertises and dials canonical (listening) addresses instead of ephemeral connection ports, and skips both ourselves and peers we are already connected to in either direction, so transitive discovery works without opening duplicate connections.
- `Tcp::connect` registers the peer synchronously and returns immediately — the prior busy-poll registration wait was removed — and now refuses self-connections and connections beyond `max_peers`.
- **Breaking:** rate-limiter construction is now fallible — `RateLimiter::new`/`with_params` are replaced by `RateLimiter::try_new`/`try_with_params`, which return `Result` instead of panicking on invalid parameters.
- **Breaking:** `Tcp::set_rate_limit` and `Gossip::new` now return `Result` rather than constructing infallibly.
- **Breaking:** `NodeConfig` is validated on deserialization, so an out-of-range configuration can no longer be produced by `serde`; validation logic is centralized in `NodeConfig::validate`.
- **Breaking (format):** all `Duration` configuration fields now serialize uniformly via serde's default representation; the seconds-only encoding previously applied to `AntiEntropyConfig::interval` is gone, so serialized configs from `1.0.0` that used the bare-seconds form no longer round-trip.
- Collapsed the `RwLock<Tcp>` and `RwLock<Gossip>` nesting to lock-free `Arc` sharing with interior-mutable state, removing the read guard the receive loop previously held across `recv().await`.
- **Breaking:** `Message::new` and `Message::with_ttl` take an explicit per-origin `sequence` argument; the process-global sequence counter is gone, so `(origin, sequence)` is now a true per-origin monotonic identifier (and the basis for the version-vector reconciliation above). The node's own broadcast counter lives in the gossip engine and is drawn only for broadcasts, keeping each origin's reconciled stream dense.
- **Breaking:** `MessageId` equality and hashing now key on `(origin, sequence)` only; `timestamp` is kept as metadata but no longer participates in identity, decoupling a message's identity from the wall clock.
- **Breaking (format):** the anti-entropy wire format changed — `Payload::AntiEntropyDigest` and `Payload::MessageRequest` now carry a per-origin version vector (`Vec<(SocketAddr, u64)>`) instead of a `Vec<MessageId>`, so `1.1.0` nodes do not reconcile with `1.0.0` nodes.
- Epidemic forwarding is now a coherent per-rumor blind variant (Demers §1.3): a newly learned rumor is forwarded once, gated by `forward_probability`, to a fanout that excludes the immediate sender and the origin so a message is never echoed straight back. The previous per-receive coin and the never-incremented forward-count cap are gone; TTL and deduplication bound propagation.
- Direct messages are no longer entered into the seen-message cache: being unicast and single-hop they are delivered on receipt and excluded from anti-entropy, which keeps each origin's broadcast sequence dense and stops direct-message content from leaking to non-recipients through the digest exchange.
- The send path now serializes each message exactly once, at the socket: the per-peer channel carries the `Message` and the writer task encodes it, removing the prior encode-in-`send` plus decode-in-writer round trip (three serializations per send reduced to one).
- The per-peer write channels and the shared inbound channel are now bounded rather than unbounded, closing a memory-exhaustion vector under load. A full write channel drops the newest frame (gossip is lossy and recovered by epidemic broadcast and anti-entropy); a full inbound channel applies backpressure to the connection readers.
- Shutdown is now real: the listener and per-connection tasks are signalled and awaited — queued goodbyes are flushed first — instead of the node sleeping a fixed 500 ms while the accept loop kept running. Peer eviction (`Tcp::disconnect`) now aborts the connection's lingering reader, which Phase 3 had left detached.
- **Breaking:** `PeerId` is now a node's Ed25519 public key (`PeerId([u8; 32])`) rather than its socket address; `From<SocketAddr> for PeerId` is gone, and `PeerId::Display` renders a hex key prefix. Origin-spoofing is therefore detectable: identity is the key, not the address.
- **Breaking:** `PeerInfo` now describes a connection by address — its `id: PeerId` field is replaced by `addr: SocketAddr`, `PeerInfo::new` and `Peer::new` take a `SocketAddr`, and `Peer::id()` is renamed `Peer::addr()`.
- **Breaking (format):** every `Message` now carries the origin's `origin_key` (Ed25519 public key) and a `signature`; the wire format changed, so `1.1.0` nodes do not interoperate with `1.0.0` nodes. Unsigned messages (those built by `Message::new`/`with_ttl`, which now exist only for tests and framing) are rejected on receipt.
- Anti-entropy now signs the digests, requests, and repair responses it sends, and independently authenticates every repaired message it receives, so the repair path cannot be used to inject forged or tampered messages.

### Removed

- **Breaking:** the unused `Transport` trait (and the `async-trait` dependency it required).
- **Breaking:** the never-sent `Payload::PeerDiscovery` and `Payload::PeerAnnouncement` variants.
- **Breaking:** the public `serde_duration` module, superseded by the uniform `Duration` representation above.
- **Breaking:** `EpidemicConfig::max_forwards` — redundant with the TTL bound and per-node deduplication (it could never exceed one forward per node), superseded by the blind-variant forwarding above.
- **Breaking:** `MessageEntry::forward_count`, now that forwarding no longer tracks a per-node count.
- **Breaking:** the public `MessageDigest` type, superseded by the version-vector reconciliation.
- **Breaking:** the optional `crypto` feature flag and the `blake3` dependency. Authenticity is always on (Ed25519 via `ed25519-dalek`, now a non-optional dependency), so the `#[cfg(feature = "crypto")]` seams, the dead `NodeConfig::enable_signing` field and its builder method, and the placeholder `Option<Vec<u8>>` signature field are all gone, replaced by the real signed-message type.

### Fixed

- Anti-entropy reconciliation, which was inverted and repaired nothing in `1.0.0` (the digest difference ran opposite to its documentation and both call sites, so a node tried to send messages it lacked and request messages it already held), is rebuilt as a correct scuttlebutt-style push-pull exchange: each node advertises a per-origin version vector (`origin -> lowest sequence still needed`) and pushes back every message the peer lacks, then requests the same in return, settling in one bounded round. A `missed-by-epidemic, recovered-by-anti-entropy` integration test proves a node with forwarding disabled still converges through the digest exchange alone.
- Anti-entropy repair responses are bounded to the frame limit: instead of packing every missing message into one `MessageResponse` (which could exceed `max_message_size` and have the codec tear the connection down), responses are chunked into frames that each fit within the limit, and a single message too large to fit even alone is dropped rather than emitted as an undeliverable frame.
- `max_peers` is now enforced at connection time on both the inbound (accept) and outbound (connect) paths, with a self-connection guard; the documented bound was previously never applied.
- The `Heartbeat` payload is now handled instead of silently falling through, so heartbeats keep otherwise-idle peers alive.
- Stale and unhealthy peers are now evicted: the maintenance loop marks a peer `Stale` after `peer_timeout` of silence and disconnects it on the consecutive-failure or prolonged-silence thresholds, evicting the lowest-`health_score` peers if the cap is ever exceeded. `health_score` and the `Stale`/`Disconnected` states were previously unreachable, and `canonical_addrs` is now pruned when a peer disconnects.
- Normal peer disconnects are logged at `debug` rather than `error`.
- Removed production panics: the backwards-clock `expect` in `MessageId::new`, the invalid-config `expect` in the rate limiter, the address-parse `expect` in `NodeConfig::default`, and the `unwrap`/`expect` chains in the CLI (including a status-line fallback that always panicked).
- Replaced lossy `as` casts with checked conversions throughout the core (frame-length, timestamp, and health-score arithmetic).
- Corrected the documented default message-size limit from 1 MB to the actual 10 MB in `docs/architecture.md` and `docs/protocol.md`.

## [1.0.0] - 2025-11-10

This is a major rewrite representing the first production-ready release of Grapevine.

### Added

#### Core Protocol

- **Epidemic Broadcast Protocol**: Probabilistic message dissemination with configurable forward probability (70% default) and max forwards (5 default)
- **Anti-Entropy Mechanism**: Periodic digest exchange and message repair (30s interval) ensures eventual consistency
- **Direct Messaging**: Point-to-point messaging between specific peers without gossip propagation
- **Message Deduplication**: Time-based message cache with TTL-based eviction (5 minute default) prevents duplicates
- **Graceful Shutdown**: Phased shutdown process with goodbye notifications to all connected peers
- **Peer State Management**: Automatic health monitoring with state machine (Connecting -> Connected -> Stale -> Disconnected)

#### Transport Layer

- **TCP Transport**: Async TCP transport with length-prefixed message framing
- **Message Codec**: Efficient binary serialization using `bincode` with configurable max frame size (10 MB default)
- **Rate Limiting**: Per-peer token bucket rate limiter (100 capacity, 50 tokens/sec default) prevents DoS attacks
- **Connection Management**: Automatic peer registration, health checks, and connection cleanup

#### Node API

- **High-Level API**: `Node` struct provides simple async interface for library consumers
- **Configuration Builder**: `NodeConfigBuilder` with fluent API for easy configuration
- **Message Handlers**: Callback-based message handling for application integration
- **Peer Management**: Methods to query connected peers and node status

#### CLI Application

- **Interactive REPL**: Full-featured interactive client with colored output
- **Real-Time Messages**: Live message display without interrupting user input
- **Command Shortcuts**: Quick commands (`/b`, `/s`, `/p`, `/st`, `/h`, `/q`)
- **Environment Support**: Configuration via environment variables or `.env` file
- **Commands**:
  - `/broadcast <message>` - Broadcast to all peers
  - `/send <peer> <message>` - Send direct message
  - `/peers` - List connected peers
  - `/status` - Show node status
  - `/help` - Display help
  - `/quit` - Graceful shutdown

#### Testing

- **42 Unit Tests**: Comprehensive coverage of core functionality
  - Message serialization/deserialization
  - Codec framing and validation
  - Configuration validation
  - Rate limiter behavior
  - Peer health scoring
  - Direct message handling
- **20 Integration Tests**: End-to-end testing of network behavior
  - Message broadcast and propagation
  - Direct messaging isolation
  - Node lifecycle and shutdown
  - Rate limiting enforcement
  - Multi-node mesh networks
- **3 Doc Tests**: Documentation examples verified
- **Benchmark Suite**: Performance testing for critical paths

#### Documentation

- **Comprehensive README**: Quick start guide, examples, and API overview
- **CLI Usage Guide** (`docs/client.md`): Complete interactive client documentation
- **Protocol Specification** (`docs/protocol.md`): Detailed protocol description
- **Architecture Documentation** (`docs/architecture.md`): System design and decisions
- **API Examples**: Three runnable examples showing common usage patterns
  - `simple_node.rs` - Basic node setup
  - `multi_node_cluster.rs` - Multi-node demonstration
  - `custom_config.rs` - Advanced configuration

#### Configuration

- **Flexible Configuration**: Builder pattern with sensible defaults
- **Environment Variables**: Support for `BIND_HOST`, `BIND_PORT`, `BOOTSTRAP_PEERS`, etc.
- **Validation**: Comprehensive input validation with clear error messages
- **Tunable Parameters**:
  - Gossip interval (5s default)
  - Fan-out factor (3 default)
  - Max peers (50 default)
  - Max message size (10 MB default)
  - Peer timeout (30s default)
  - Connection timeout (10s default)

#### CI/CD

- **GitHub Actions Workflows**: Modern CI with matrix testing
- **Security Audits**: Automated security vulnerability scanning
- **Pre-commit Hooks**: Code quality checks before commit
- **Multiple Rust Versions**: Testing across stable and nightly

### Changed

#### Breaking Changes (v0.1.3 -> v1.0.0)

- **Complete API Redesign**: Previous simple node implementation replaced with full gossip protocol
- **Async/Await**: Changed from synchronous to asynchronous API using `tokio`
- **Configuration System**: New builder pattern replaces old configuration
- **Message Format**: New binary message format with length-prefixed framing
- **Dependencies**: Updated to Rust 2024 Edition with modern dependencies

#### Code Quality Improvements

- **Zero Magic Numbers**: All constants properly named and documented
- **Consistent Logging**: Layered logging (INFO for protocol, DEBUG for transport)
- **Comprehensive Errors**: Rich error types with source chains
- **No Unsafe Code**: Memory safe and thread safe throughout
- **Clippy Clean**: Zero warnings with strict lints
- **Formatted Code**: Consistent style with rustfmt

#### Project Structure

- Reorganized into modules: `core`, `protocol`, `transport`, `node`
- Separated concerns with clear boundaries between layers
- Improved modularity and testability

### Removed

- **Old `Node` Implementation**: Simple synchronous node replaced
- **Deprecated APIs**: All v0.1.x APIs removed (breaking change)
- **Legacy Configuration**: Old configuration system removed

#### Updated

- Rust edition: 2021 -> 2024
- All dependencies updated to latest stable versions

### Migration Guide

For users upgrading from v0.1.x to v1.0.0:

```rust
// Before (v0.1.x)
use grapevine::Node;
let node = Node::new("127.0.0.1:8000");

// After (v1.0.0)
use grapevine::{Node, NodeConfigBuilder};
use std::time::Duration;

let config = NodeConfigBuilder::new()
    .bind_addr("127.0.0.1:8000".parse()?)
    .gossip_interval(Duration::from_secs(5))
    .fanout(3)
    .build()?;

let node = Node::new(config).await?;

// Set up message handler
node.on_message(|origin, data| {
    println!("Received from {origin}: {data:?}");
}).await;

// Start the node
node.start().await?;

// Use the node
node.broadcast(Bytes::from("Hello!")).await?;
```

---

## [0.1.3] - Prior to fork

Previous development version before v1.0.0 rewrite.

[1.0.0]: https://github.com/kobby-pentangeli/grapevine/compare/v0.1.3...v1.0.0
[0.1.3]: https://github.com/kobby-pentangeli/grapevine/releases/tag/v0.1.3
