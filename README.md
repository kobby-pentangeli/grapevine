# Grapevine

[![Crates.io](https://img.shields.io/crates/v/grapevine.svg)](https://crates.io/crates/grapevine)
[![Documentation](https://docs.rs/grapevine/badge.svg)](https://docs.rs/grapevine)
[![License](https://img.shields.io/crates/l/grapevine.svg)](https://github.com/kobby-pentangeli/grapevine#license)

A modern, asynchronous peer-to-peer gossip protocol library for Rust.

## Features

- **Async/await** - Built on Tokio for high-performance async I/O
- **Pluggable transports** - TCP by default, QUIC support via feature flag
- **Highly configurable** - Fine-tune gossip parameters for your use case
- **Secure** - Optional message signing and encryption

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
grapevine = "1.0"
tokio = { version = "1", features = ["full"] }
bytes = "1"
```

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
        println!("Received from {}: {:?}", origin, data);
    }).await;

    // Start the node
    node.start().await?;

    // Broadcast a message
    node.broadcast(Bytes::from("Hello, gossip!")).await?;

    // Keep running
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

## Feature Flags

- `quic` - Enable QUIC transport support
- `crypto` - Enable message signing and verification
- `json-config` - JSON configuration file support (default)
- `yaml-config` - YAML configuration file support
- `toml-config` - TOML configuration file support
- `full` - Enable all features

## Architecture

Grapevine implements a push-based gossip protocol with the following components:

- **Epidemic Broadcast**: Probabilistic message dissemination for efficient network coverage
- **Anti-Entropy**: Periodic synchronization to ensure eventual consistency
- **Peer Management**: Automatic peer discovery and health monitoring
- **Message Deduplication**: Efficient tracking of seen messages
- **Configurable Fan-out**: Control gossip spread vs. network load

## Performance

Benchmarks on Apple M1 Pro:

```bash
broadcast_10_nodes      time:   [1.23 ms 1.25 ms 1.27 ms]
broadcast_50_nodes      time:   [4.56 ms 4.61 ms 4.67 ms]
broadcast_100_nodes     time:   [8.12 ms 8.21 ms 8.31 ms]
```

Run benchmarks yourself:

```bash
cargo bench
```

## Testing

```bash
# Run all tests
cargo test --all-features

# Run integration tests only
cargo test --test integration

# Run with logging
RUST_LOG=debug cargo test
```

## Examples

See the [examples](examples/) directory:

- [`simple_node.rs`](examples/simple_node.rs) - Single node setup
- [`multi_node_cluster.rs`](examples/multi_node_cluster.rs) - Multi-node cluster
- [`custom_config.rs`](examples/custom_config.rs) - Custom configuration

Run an example:

```bash
cargo run --example simple_node
```

## Contributing

Contributions are welcome! Please read our [Contributing Guidelines](CONTRIBUTING.md) and [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
