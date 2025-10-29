//! Example demonstrating custom configuration.
//!
//! Run with: RUST_LOG=info cargo run --example custom_config

use std::time::Duration;

use grapevine::{Node, NodeConfigBuilder, TransportConfig};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Build custom configuration
    let config = NodeConfigBuilder::new()
        .bind_addr("127.0.0.1:9000".parse()?)
        .gossip_interval(Duration::from_secs(3))
        .fanout(5)
        .max_peers(100)
        .max_message_size(2 * 1024 * 1024) // 2 MB
        .peer_timeout(Duration::from_secs(60))
        .connection_timeout(Duration::from_secs(15))
        .transport(TransportConfig::Tcp)
        .build()?;

    info!("Configuration:");
    info!("  Bind address: {}", config.bind_addr);
    info!("  Gossip interval: {:?}", config.gossip_interval);
    info!("  Fan-out: {}", config.fanout);
    info!("  Max peers: {}", config.max_peers);
    info!("  Max message size: {} bytes", config.max_message_size);

    let node = Node::new(config).await?;
    node.start().await?;

    let addr = node.local_addr().await.expect("No local address");
    info!("Node started on {addr}");

    tokio::signal::ctrl_c().await?;
    node.shutdown().await?;

    Ok(())
}
