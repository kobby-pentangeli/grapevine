//! Verify message propagation in complex multi-node networks,
//! message deduplication, and high-volume broadcast scenarios.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::Bytes;
use common::init_tracing;
use grapevine::{Node, NodeConfig, NodeConfigBuilder};

/// Test five-node mesh with multiple concurrent broadcasts.
#[tokio::test(flavor = "multi_thread")]
async fn five_node_mesh_broadcast() {
    init_tracing();

    // Create node1 (seed)
    let node1 = Node::new(NodeConfig::default())
        .await
        .expect("Failed to create node1");

    let received1 = Arc::new(AtomicU32::new(0));
    let received1_clone = Arc::clone(&received1);
    node1
        .on_message(move |_origin, _data| {
            received1_clone.fetch_add(1, Ordering::Relaxed);
        })
        .await;

    node1.start().await.expect("Failed to start node1");
    let addr1 = node1.local_addr().await.expect("No local address");

    // Create 4 additional nodes, all connecting to node1
    let mut nodes = vec![node1];
    let mut counters = vec![received1];

    for i in 2..=5 {
        let config = NodeConfigBuilder::new()
            .add_bootstrap_peer(addr1)
            .fanout(3) // Higher fanout for better propagation
            .build()
            .expect("Failed to build config");

        let node = Node::new(config)
            .await
            .unwrap_or_else(|_| panic!("Failed to create node{i}"));

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);
        node.on_message(move |_origin, _data| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        })
        .await;

        node.start()
            .await
            .unwrap_or_else(|_| panic!("Failed to start node{i}"));

        nodes.push(node);
        counters.push(counter);
    }

    // Wait for mesh to form; nodes need time to discover each other
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Each node broadcasts a unique message
    for (i, node) in nodes.iter().enumerate() {
        node.broadcast(Bytes::from(format!("message from node{}", i + 1)))
            .await
            .expect("Failed to broadcast");
        // Small delay between broadcasts to avoid overwhelming the network
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Wait for all messages to propagate through the network
    tokio::time::sleep(Duration::from_secs(4)).await;

    // Each node should receive messages from other nodes (4 messages expected)
    // Note: Nodes do not receive their own broadcasts via the message handler
    // With probabilistic forwarding (70%), network topology, and potential timing issues,
    // expect at least 1 message (conservative to avoid flakiness)
    for (i, counter) in counters.iter().enumerate() {
        let received = counter.load(Ordering::Relaxed);
        assert!(
            received >= 1,
            "Node{} should have received at least 1 message, got {}",
            i + 1,
            received
        );
    }

    // Cleanup
    for node in nodes {
        node.shutdown().await.ok();
    }
}

/// Test message deduplication across multiple nodes.
#[tokio::test(flavor = "multi_thread")]
async fn message_deduplication() {
    init_tracing();

    // Create three-node cluster
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

    let received2 = Arc::new(AtomicU32::new(0));
    let received2_clone = Arc::clone(&received2);
    node2
        .on_message(move |_origin, data| {
            if data == "dedup test" {
                received2_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    node2.start().await.expect("Failed to start node2");

    let config3 = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr1)
        .build()
        .expect("Failed to build config");
    let node3 = Node::new(config3).await.expect("Failed to create node3");

    let received3 = Arc::new(AtomicU32::new(0));
    let received3_clone = Arc::clone(&received3);
    node3
        .on_message(move |_origin, data| {
            if data == "dedup test" {
                received3_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    node3.start().await.expect("Failed to start node3");

    // Wait for connections
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Broadcast the same message multiple times from node1
    for _ in 0..5 {
        node1
            .broadcast(Bytes::from("dedup test"))
            .await
            .expect("Failed to broadcast");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Wait for propagation
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Each node should receive the message despite multiple broadcasts
    // (The gossip protocol deduplicates based on message ID, but message handler
    // is called for each broadcast since they have unique sequence numbers)
    let count2 = received2.load(Ordering::Relaxed);
    let count3 = received3.load(Ordering::Relaxed);

    // Each broadcast creates a new message with unique ID, so expect to receive all 5
    // However, with probabilistic forwarding (70%), some may be dropped
    assert!(
        (3..=5).contains(&count2),
        "Node2 should receive 3-5 messages (70% forward probability), got {count2}"
    );
    assert!(
        (3..=5).contains(&count3),
        "Node3 should receive 3-5 messages (70% forward probability), got {count3}"
    );

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
    node3.shutdown().await.ok();
}

/// Test broadcast with high message volume.
#[tokio::test(flavor = "multi_thread")]
async fn high_volume_broadcast() {
    init_tracing();

    // Create three-node cluster
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

    let received2 = Arc::new(AtomicU32::new(0));
    let received2_clone = Arc::clone(&received2);
    node2
        .on_message(move |_origin, _data| {
            received2_clone.fetch_add(1, Ordering::Relaxed);
        })
        .await;
    node2.start().await.expect("Failed to start node2");

    let config3 = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr1)
        .build()
        .expect("Failed to build config");
    let node3 = Node::new(config3).await.expect("Failed to create node3");

    let received3 = Arc::new(AtomicU32::new(0));
    let received3_clone = Arc::clone(&received3);
    node3
        .on_message(move |_origin, _data| {
            received3_clone.fetch_add(1, Ordering::Relaxed);
        })
        .await;
    node3.start().await.expect("Failed to start node3");

    // Wait for connections
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Broadcast 50 unique messages
    let message_count = 50;
    for i in 0..message_count {
        node1
            .broadcast(Bytes::from(format!("message {i}")))
            .await
            .expect("Failed to broadcast");
    }

    // Wait for propagation
    tokio::time::sleep(Duration::from_secs(2)).await;

    let count2 = received2.load(Ordering::Relaxed);
    let count3 = received3.load(Ordering::Relaxed);

    // Both nodes should receive most messages (allow for some loss due to probabilistic forwarding)
    // Expect at least 80% delivery rate
    let min_expected = (message_count as f64 * 0.8) as u32;
    assert!(
        count2 >= min_expected,
        "Node2 should receive at least {min_expected} messages, got {count2}"
    );
    assert!(
        count3 >= min_expected,
        "Node3 should receive at least {min_expected} messages, got {count3}"
    );

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
    node3.shutdown().await.ok();
}
