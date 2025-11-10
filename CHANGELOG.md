# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
