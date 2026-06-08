# Grapevine

[![Crates.io](https://img.shields.io/crates/v/grapevine.svg)](https://crates.io/crates/grapevine)
[![Documentation](https://docs.rs/grapevine/badge.svg)](https://docs.rs/grapevine)
[![CI](https://github.com/kobby-pentangeli/grapevine/workflows/CI/badge.svg)](https://github.com/kobby-pentangeli/grapevine/actions)
[![License](https://img.shields.io/crates/l/grapevine.svg)](https://github.com/kobby-pentangeli/grapevine#license)

A modern, asynchronous peer-to-peer gossip protocol library and application.

## Features

- **Async/await** - Built on Tokio for high-performance async I/O
- **Authenticated messages** - Every message is Ed25519-signed by its origin and verified on receipt
- **Epidemic broadcast** - Probabilistic message forwarding for efficient network coverage
- **Anti-entropy** - Periodic synchronization ensures eventual consistency
- **Rate limiting** - Per-peer token bucket rate limiting prevents DoS attacks
- **Highly configurable** - Fine-tune gossip parameters for your use case
- **Zero unsafe code** - Memory safe and thread safe

## Installation

### As a Library

To use Grapevine in your Rust project:

```bash
cargo add grapevine
```

Or add manually to your `Cargo.toml`:

```toml
[dependencies]
grapevine = "1.1"
tokio = { version = "1", features = ["full"] }
bytes = "1"
```

### As a CLI Application

To install the standalone gossip client binary:

```bash
cargo install grapevine
```

Then run:

```bash
grapevine --help
```

## Quick Start

### Basic Example

```rust
use grapevine::{Node, NodeConfig};
use bytes::Bytes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a node with default configuration
    let config = NodeConfig::default();
    let node = Node::new(config).await?;

    // Set up message handler
    node.on_message(|origin, data| {
        println!("Received from {origin}: {data:?}");
    }).await;

    // Start the node
    node.start().await?;

    // Broadcast a message
    node.broadcast(Bytes::from("Hello, gossip!")).await?;

    // Keep running until explicit shutdown
    tokio::signal::ctrl_c().await?;
    node.shutdown().await?;

    Ok(())
}
```

### Multi-Node Cluster

```rust
use grapevine::{Node, NodeConfig, NodeConfigBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create first node
    let node1 = Node::new(NodeConfig::default()).await?;
    node1.start().await?;
    let addr1 = node1.local_addr().await.unwrap();

    // Create second node, bootstrapping from first
    let config2 = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr1)
        .fanout(5)
        .build()?;
    let node2 = Node::new(config2).await?;
    node2.start().await?;

    // Nodes will automatically discover and connect to each other

    Ok(())
}
```

## Configuration

Grapevine is highly configurable. See [`NodeConfig`](https://docs.rs/grapevine/latest/grapevine/config/struct.NodeConfig.html) for all options:

```rust
use grapevine::NodeConfigBuilder;
use std::time::Duration;

let config = NodeConfigBuilder::new()
    .bind_addr("127.0.0.1:8000".parse().unwrap())
    .gossip_interval(Duration::from_secs(5))
    .fanout(3)
    .max_peers(50)
    .max_message_size(1024 * 1024)
    .build()?;
```

## Message Authenticity

Each node holds an Ed25519 keypair (its `PeerId` is the public key), signs every message it originates over a domain-separated encoding of the immutable `(origin, sequence, payload)`, and embeds the public key. Recipients verify the signature and pin each origin address to the key it first presented (trust-on-first-use), so a peer cannot forge a message attributed to a pinned origin. This provides integrity and origin authenticity but **not** confidentiality; see [`docs/protocol.md`](docs/protocol.md#message-authenticity) for the full threat model.

Note: QUIC transport and TLS (for confidentiality) are planned for a future release.

## Architecture

Grapevine implements a push-based gossip protocol with the following components:

- **Message Authenticity**: Ed25519 signing and verification of every message, with trust-on-first-use origin pinning
- **Epidemic Broadcast**: Probabilistic rumor mongering (blind variant; default 70% forward probability)
- **Anti-Entropy**: Periodic version-vector reconciliation and repair (every 30s) ensures eventual consistency
- **Peer Management**: Automatic health monitoring with state machine (Connecting => Connected => Stale => Disconnected)
- **Rate Limiting**: Per-peer token bucket (100 capacity, 50 tokens/sec) prevents DoS attacks
- **Message Deduplication**: Time-based eviction (5 minute TTL) prevents duplicates
- **Graceful Shutdown**: Phased shutdown with goodbye notifications to peers

See [Architecture Documentation](docs/architecture.md) for details.

## Testing

```bash
# Run all tests
cargo test --all-features

# Run integration tests only
cargo test --test integration

# Run with logging
RUST_LOG=debug cargo test
```

## CLI Usage

Grapevine includes a standalone binary for running gossip nodes. For a complete guide on starting nodes, joining networks, broadcasting messages, and more, see the **[CLI Usage Guide](docs/client.md)**.

### Environment Variables

For a straightforward run, first copy `.env.example` to `.env` and customize:

```bash
cp .env.example .env
# Edit .env with your configuration
cargo run
```

### CLI Arguments

```bash
-H, --host <HOST>                Host to bind to [env: BIND_HOST] [default: 127.0.0.1]
-p, --port <PORT>                Port to listen on [env: BIND_PORT] [default: 8000]
-b, --peer <PEER>                Bootstrap peer addresses [env: BOOTSTRAP_PEERS]
-g, --gossip-interval <SECS>     Gossip interval in seconds [env: GOSSIP_INTERVAL_SECS] [default: 5]
-f, --fanout <FANOUT>            Fan-out factor [env: FANOUT] [default: 3]
-m, --max-peers <MAX_PEERS>      Maximum number of peers [env: MAX_PEERS] [default: 50]
-l, --log-level <LEVEL>          Log level (trace, debug, info, warn, error) [env: RUST_LOG] [default: info]
```

### Common Operations

```bash
# Start a seed node
cargo run

# Join the network from another terminal
cargo run -- --port 8001 --peer 127.0.0.1:8000

# Join with multiple bootstrap peers
cargo run -- --port 8002 \
  --peer 127.0.0.1:8000,127.0.0.1:8001

# Start with custom configuration
cargo run -- \
  --host 0.0.0.0 \
  --port 9000 \
  --fanout 5 \
  --gossip-interval 3

# Use environment variables
BIND_HOST=0.0.0.0 BIND_PORT=9000 cargo run

# Enable debug logging
cargo run -- --log-level debug

# Graceful shutdown
# Press Ctrl+C to send goodbye messages and cleanly exit
```

See **[docs/client.md](docs/client.md)** for:

- Step-by-step setup instructions
- Multi-node cluster examples
- Network tuning parameters
- Troubleshooting common issues

## Examples

See the [examples](examples/) directory:

- [`simple_node.rs`](examples/simple_node.rs) - Single node setup
- [`multi_node_cluster.rs`](examples/multi_node_cluster.rs) - Multi-node cluster
- [`custom_config.rs`](examples/custom_config.rs) - Custom configuration

Run an example:

```bash
RUST_LOG=info cargo run --example simple_node
```

## Contributing

Contributions are welcome! Please read our [Contributing Guidelines](CONTRIBUTING.md) and [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
