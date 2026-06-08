//! Verify message propagation in complex multi-node networks,
//! message deduplication, and high-volume broadcast scenarios.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::Bytes;
use common::{READY_TIMEOUT, init_tracing, wait_for_peers, wait_until};
use grapevine::{AntiEntropyConfig, Node, NodeConfig, NodeConfigBuilder};

/// Frequent reconciliation so a message dropped by epidemic push (probabilistic
/// forwarding, or a write channel shedding load under a burst) is repaired
/// within the test window rather than at the 30s default interval.
fn brisk_anti_entropy() -> AntiEntropyConfig {
    AntiEntropyConfig {
        interval: Duration::from_millis(500),
        fanout: 3,
        enabled: true,
    }
}

/// Test five-node mesh with multiple concurrent broadcasts.
#[tokio::test(flavor = "multi_thread")]
async fn five_node_mesh_broadcast() {
    init_tracing();

    let node1 = Node::new(
        NodeConfigBuilder::new()
            .anti_entropy(brisk_anti_entropy())
            .build()
            .expect("Failed to build node1 config"),
    )
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

    let mut nodes = vec![node1];
    let mut counters = vec![received1];
    let mut addresses = vec![addr1];

    for i in 2..=5 {
        let mut config_builder = NodeConfigBuilder::new()
            .add_bootstrap_peer(addr1)
            .anti_entropy(brisk_anti_entropy())
            .fanout(3);

        if i > 2 {
            config_builder = config_builder.add_bootstrap_peer(addresses[i - 3]);
        }

        let config = config_builder.build().expect("Failed to build config");

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

        let addr = node.local_addr().await.expect("No local address");
        addresses.push(addr);
        nodes.push(node);
        counters.push(counter);
    }

    wait_for_peers(&nodes[0], 4, "node1 holds the four leaves").await;

    for (i, node) in nodes.iter().enumerate() {
        node.broadcast(Bytes::from(format!("message from node{}", i + 1)))
            .await
            .expect("Failed to broadcast");
    }

    wait_until(
        "every node to receive a message from a peer",
        READY_TIMEOUT,
        || counters.iter().all(|c| c.load(Ordering::Relaxed) >= 1),
    )
    .await;

    for node in nodes {
        node.shutdown().await.ok();
    }
}

/// Test message deduplication across multiple nodes.
#[tokio::test(flavor = "multi_thread")]
async fn message_deduplication() {
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

    wait_for_peers(&node1, 2, "node1 holds both leaves").await;

    for _ in 0..5 {
        node1
            .broadcast(Bytes::from("dedup test"))
            .await
            .expect("Failed to broadcast");
    }

    wait_until(
        "both leaves to deliver all five distinct broadcasts",
        READY_TIMEOUT,
        || received2.load(Ordering::Relaxed) == 5 && received3.load(Ordering::Relaxed) == 5,
    )
    .await;

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
    node3.shutdown().await.ok();
}

/// Test broadcast with high message volume.
#[tokio::test(flavor = "multi_thread")]
async fn high_volume_broadcast() {
    init_tracing();

    let node1 = Node::new(
        NodeConfigBuilder::new()
            .anti_entropy(brisk_anti_entropy())
            .build()
            .expect("Failed to build node1 config"),
    )
    .await
    .expect("Failed to create node1");
    node1.start().await.expect("Failed to start node1");
    let addr1 = node1.local_addr().await.expect("No local address");

    let config2 = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr1)
        .anti_entropy(brisk_anti_entropy())
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
        .anti_entropy(brisk_anti_entropy())
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

    wait_for_peers(&node1, 2, "node1 holds both leaves").await;

    let message_count = 50u32;
    for i in 0..message_count {
        node1
            .broadcast(Bytes::from(format!("message {i}")))
            .await
            .expect("Failed to broadcast");
    }

    let min_expected = message_count * 4 / 5; // 80% delivery
    wait_until(
        "both leaves to converge to at least 80% of the burst",
        READY_TIMEOUT,
        || {
            received2.load(Ordering::Relaxed) >= min_expected
                && received3.load(Ordering::Relaxed) >= min_expected
        },
    )
    .await;

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
    node3.shutdown().await.ok();
}
