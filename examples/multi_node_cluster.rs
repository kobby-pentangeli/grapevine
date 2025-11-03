//! Multi-node cluster example.
//!
//! Creates a cluster of 5 nodes and demonstrates message propagation.
//!
//! Run with: RUST_LOG=info cargo run --example multi_node_cluster

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use grapevine::{Node, NodeConfig, NodeConfigBuilder};
use tokio::sync::Mutex;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    info!("Creating cluster of 5 nodes");

    let mut nodes = Vec::new();

    // Create first node
    let node1 = Node::new(NodeConfig::default()).await?;
    node1.start().await?;
    let addr1 = node1.local_addr().await.unwrap();
    info!("Node 1 started on {addr1}");

    let received_count = Arc::new(Mutex::new(0u32));

    // Set handler for node 1
    {
        let count = Arc::clone(&received_count);
        node1
            .on_message(move |origin, data| {
                info!("[Node 1] Received from {origin}: {data:?}");
                let count = Arc::clone(&count);
                tokio::spawn(async move {
                    let mut c = count.lock().await;
                    *c += 1;
                });
            })
            .await;
    }

    nodes.push(node1);

    // Create remaining nodes
    for i in 2..=5 {
        let config = NodeConfigBuilder::new()
            .add_bootstrap_peer(addr1)
            .gossip_interval(Duration::from_secs(2))
            .fanout(3)
            .build()?;

        let node = Node::new(config).await?;
        node.start().await?;

        let addr = node.local_addr().await.unwrap();
        info!("Node {i} started on {addr}");

        let count = Arc::clone(&received_count);
        let node_id = i;
        node.on_message(move |origin, data| {
            info!("[Node {node_id}] Received from {origin}: {data:?}");
            let count = Arc::clone(&count);
            tokio::spawn(async move {
                let mut c = count.lock().await;
                *c += 1;
            });
        })
        .await;

        nodes.push(node);
    }

    // Wait for connections to stabilize
    info!("Waiting for peer connections to stabilize...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Check peer connections
    info!("Checking peer connections:");
    for (i, node) in nodes.iter().enumerate() {
        let peers = node.peers().await;
        info!("  Node {} has {} peers: {:?}", i + 1, peers.len(), peers);
    }

    // Verify nodes have peers before broadcasting
    let mut total_peers = 0;
    for node in &nodes {
        total_peers += node.peers().await.len();
    }

    if total_peers == 0 {
        eprintln!("Warning: No peer connections established. Message propagation will fail.");
    } else {
        info!("Total peer connections across cluster: {total_peers}");
    }

    // Broadcast from first node
    info!("Broadcasting message from Node 1");
    nodes[0]
        .broadcast(Bytes::from("Hello from the cluster!"))
        .await?;

    // Wait for propagation
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Check how many nodes received the message
    let count = *received_count.lock().await;
    info!("Message received by {count} nodes (expected: 4, excluding sender)");

    if count >= 3 {
        info!("Success! Message propagated to majority of nodes.");
    } else {
        eprintln!("Warning: Message only reached {count} nodes. Expected at least 3.");
    }

    info!("Cluster example complete. Press Ctrl-C to exit.");
    tokio::signal::ctrl_c().await?;

    // Cleanup
    for (i, node) in nodes.iter().enumerate() {
        info!("Shutting down node {}", i + 1);
        node.shutdown().await?;
    }

    Ok(())
}
