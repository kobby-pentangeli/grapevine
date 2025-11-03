//! Simple single-node example.
//!
//! Run with: `RUST_LOG=info cargo run --example simple_node`
//! Shutdown with `Control+C (^C)`

use bytes::Bytes;
use grapevine::{Node, NodeConfig};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("Starting simple node example");

    // Create node
    let config = NodeConfig::default();
    let node = Node::new(config).await?;

    // Set message handler
    node.on_message(|origin, data| {
        info!("Received message from {origin}: {data:?}");
    })
    .await;

    // Start node
    node.start().await?;
    let addr = node.local_addr().await.expect("No local address");
    info!("Node listening on {addr}");

    // Broadcast a message
    node.broadcast(Bytes::from("Hello from simple node!"))
        .await?;

    // Keep running until Ctrl-C
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    node.shutdown().await?;

    Ok(())
}
