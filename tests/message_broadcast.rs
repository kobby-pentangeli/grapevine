//! Test message broadcast for small clusters (2-3 nodes) in simple network topologies.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bytes::Bytes;
use common::{READY_TIMEOUT, init_tracing, wait_for_peers, wait_until};
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

    wait_for_peers(&node1, 1, "node1 connects to node2").await;

    node1
        .broadcast(Bytes::from("test message"))
        .await
        .expect("Failed to broadcast");

    wait_until("node2 to receive the broadcast", READY_TIMEOUT, || {
        received.load(Ordering::Relaxed) >= 1
    })
    .await;

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
}

/// Test three-node cluster with message propagation.
#[tokio::test(flavor = "multi_thread")]
async fn three_node_broadcast_propagation() {
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
            if data == "propagation test" {
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
            if data == "propagation test" {
                received3_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    node3.start().await.expect("Failed to start node3");

    wait_for_peers(&node1, 2, "node1 holds both leaves").await;

    node1
        .broadcast(Bytes::from("propagation test"))
        .await
        .expect("Failed to broadcast");

    wait_until(
        "both leaves to receive the broadcast",
        READY_TIMEOUT,
        || received2.load(Ordering::Relaxed) >= 1 && received3.load(Ordering::Relaxed) >= 1,
    )
    .await;

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
    node3.shutdown().await.ok();
}
