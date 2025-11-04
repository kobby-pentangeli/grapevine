//! Test message broadcast for small clusters (2-3 nodes) in simple network topologies.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::Bytes;
use common::init_tracing;
use grapevine::{Node, NodeConfig, NodeConfigBuilder};

/// Test message broadcast and reception between two nodes.
#[tokio::test(flavor = "multi_thread")]
async fn two_node_message_broadcast() {
    init_tracing();

    let node1 = Node::new(NodeConfig::default())
        .await
        .expect("Failed to create node1");
    node1.start().await.expect("Failed to start node1");

    let addr1 = node1.local_addr().await.expect("No local address");

    let config2 = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr1)
        .build()
        .expect("Failed to build config");

    let node2 = Node::new(config2).await.expect("Failed to create node2");

    let received = Arc::new(AtomicU32::new(0));
    let received_clone = Arc::clone(&received);

    node2
        .on_message(move |_origin, data| {
            if data == "test message" {
                received_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;

    node2.start().await.expect("Failed to start node2");

    tokio::time::sleep(Duration::from_millis(200)).await;

    node1
        .broadcast(Bytes::from("test message"))
        .await
        .expect("Failed to broadcast");

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        received.load(Ordering::Relaxed) >= 1,
        "Node2 should have received at least one message"
    );

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
}

/// Test three-node cluster with message propagation.
#[tokio::test(flavor = "multi_thread")]
async fn three_node_broadcast_propagation() {
    init_tracing();

    // Create node1 (seed)
    let node1 = Node::new(NodeConfig::default())
        .await
        .expect("Failed to create node1");
    node1.start().await.expect("Failed to start node1");
    let addr1 = node1.local_addr().await.expect("No local address");

    // Create node2 (connects to node1)
    let config2 = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr1)
        .build()
        .expect("Failed to build config");
    let node2 = Node::new(config2).await.expect("Failed to create node2");

    let received2 = Arc::new(AtomicU32::new(0));
    let received2_clone = Arc::clone(&received2);
    node2
        .on_message(move |_origin, data| {
            if data == "propagation test" {
                received2_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    node2.start().await.expect("Failed to start node2");

    // Create node3 (connects to node1)
    let config3 = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr1)
        .build()
        .expect("Failed to build config");
    let node3 = Node::new(config3).await.expect("Failed to create node3");

    let received3 = Arc::new(AtomicU32::new(0));
    let received3_clone = Arc::clone(&received3);
    node3
        .on_message(move |_origin, data| {
            if data == "propagation test" {
                received3_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    node3.start().await.expect("Failed to start node3");

    // Wait for peer discovery
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Broadcast from node1
    node1
        .broadcast(Bytes::from("propagation test"))
        .await
        .expect("Failed to broadcast");

    // Wait for propagation
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Both node2 and node3 should receive the message
    assert!(
        received2.load(Ordering::Relaxed) >= 1,
        "Node2 should have received the message"
    );
    assert!(
        received3.load(Ordering::Relaxed) >= 1,
        "Node3 should have received the message"
    );

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
    node3.shutdown().await.ok();
}
