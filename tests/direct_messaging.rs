//! Verify that messages sent via `send_to_peer` are delivered
//! only to the intended recipient and are not propagated through the gossip network.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::Bytes;
use common::init_tracing;
use grapevine::{Node, NodeConfig, NodeConfigBuilder};

/// Test sending a direct message between two nodes.
#[tokio::test(flavor = "multi_thread")]
async fn two_node_direct_message() {
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
        .on_message(move |origin, data| {
            if data == "direct message" && origin == addr1 {
                received_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;

    node2.start().await.expect("Failed to start node2");
    let addr2 = node2.local_addr().await.expect("No local address");

    tokio::time::sleep(Duration::from_millis(200)).await;

    node1
        .send_to_peer(addr2, Bytes::from("direct message"))
        .await
        .expect("Failed to send direct message");

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        received.load(Ordering::Relaxed),
        1,
        "Node2 should have received exactly one direct message"
    );

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
}

/// Test that direct messages are not propagated to other peers.
#[tokio::test(flavor = "multi_thread")]
async fn direct_message_not_propagated() {
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
        .on_message(move |origin, data| {
            if data == "private message" && origin == addr1 {
                received2_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    node2.start().await.expect("Failed to start node2");
    let addr2 = node2.local_addr().await.expect("No local address");

    let config3 = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr1)
        .build()
        .expect("Failed to build config");
    let node3 = Node::new(config3).await.expect("Failed to create node3");

    let received3 = Arc::new(AtomicU32::new(0));
    let received3_clone = Arc::clone(&received3);
    node3
        .on_message(move |_origin, data| {
            if data == "private message" {
                received3_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
    node3.start().await.expect("Failed to start node3");

    tokio::time::sleep(Duration::from_millis(300)).await;

    node1
        .send_to_peer(addr2, Bytes::from("private message"))
        .await
        .expect("Failed to send direct message");

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        received2.load(Ordering::Relaxed),
        1,
        "Node2 should have received the direct message"
    );
    assert_eq!(
        received3.load(Ordering::Relaxed),
        0,
        "Node3 should NOT have received the direct message"
    );

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
    node3.shutdown().await.ok();
}

/// Test bidirectional direct messaging between two nodes.
#[tokio::test(flavor = "multi_thread")]
async fn bidirectional_direct_messaging() {
    init_tracing();

    let node1 = Node::new(NodeConfig::default())
        .await
        .expect("Failed to create node1");

    let received1 = Arc::new(AtomicU32::new(0));
    let received1_clone = Arc::clone(&received1);
    node1
        .on_message(move |_origin, data| {
            if data == "reply" {
                received1_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;

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
            if data == "ping" {
                received2_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;

    node2.start().await.expect("Failed to start node2");
    let addr2 = node2.local_addr().await.expect("No local address");

    tokio::time::sleep(Duration::from_millis(200)).await;

    node1
        .send_to_peer(addr2, Bytes::from("ping"))
        .await
        .expect("Failed to send ping");

    tokio::time::sleep(Duration::from_millis(300)).await;

    node2
        .send_to_peer(addr1, Bytes::from("reply"))
        .await
        .expect("Failed to send reply");

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(
        received2.load(Ordering::Relaxed),
        1,
        "Node2 should have received ping"
    );
    assert_eq!(
        received1.load(Ordering::Relaxed),
        1,
        "Node1 should have received reply"
    );

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
}

/// Test sending direct message to non-existent peer returns error.
#[tokio::test(flavor = "multi_thread")]
async fn direct_message_to_nonexistent_peer() {
    init_tracing();

    let node = Node::new(NodeConfig::default())
        .await
        .expect("Failed to create node");
    node.start().await.expect("Failed to start node");

    let fake_addr = "127.0.0.1:9999".parse().unwrap();

    let result = node.send_to_peer(fake_addr, Bytes::from("test")).await;

    assert!(
        result.is_err(),
        "Sending to non-existent peer should return error"
    );

    node.shutdown().await.ok();
}

/// Test multiple sequential direct messages between peers.
#[tokio::test(flavor = "multi_thread")]
async fn multiple_sequential_direct_messages() {
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
        .on_message(move |_origin, _data| {
            received_clone.fetch_add(1, Ordering::Relaxed);
        })
        .await;

    node2.start().await.expect("Failed to start node2");
    let addr2 = node2.local_addr().await.expect("No local address");

    tokio::time::sleep(Duration::from_millis(200)).await;

    for i in 0..5 {
        node1
            .send_to_peer(addr2, Bytes::from(format!("message {i}")))
            .await
            .expect("Failed to send direct message");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        received.load(Ordering::Relaxed),
        5,
        "Node2 should have received all 5 messages"
    );

    node1.shutdown().await.ok();
    node2.shutdown().await.ok();
}

/// Test direct messaging in a four-node network verifies isolation.
#[tokio::test(flavor = "multi_thread")]
async fn direct_message_isolation_in_mesh() {
    init_tracing();

    let node1 = Node::new(NodeConfig::default())
        .await
        .expect("Failed to create node1");

    let received1 = Arc::new(AtomicU32::new(0));
    let received1_clone = Arc::clone(&received1);
    node1
        .on_message(move |_origin, data| {
            if data == "secret" {
                received1_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;

    node1.start().await.expect("Failed to start node1");
    let addr1 = node1.local_addr().await.expect("No local address");

    let mut nodes = vec![node1];
    let mut counters = vec![received1];

    for i in 2..=4 {
        let config = NodeConfigBuilder::new()
            .add_bootstrap_peer(addr1)
            .build()
            .expect("Failed to build config");

        let node = Node::new(config)
            .await
            .unwrap_or_else(|_| panic!("Failed to create node{i}"));

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);
        node.on_message(move |_origin, data| {
            if data == "secret" {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;

        node.start()
            .await
            .unwrap_or_else(|_| panic!("Failed to start node{i}"));

        nodes.push(node);
        counters.push(counter);
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    let addr2 = nodes[1].local_addr().await.expect("No local address");

    nodes[0]
        .send_to_peer(addr2, Bytes::from("secret"))
        .await
        .expect("Failed to send direct message");

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        counters[0].load(Ordering::Relaxed),
        0,
        "Node1 (sender) should not receive its own message"
    );
    assert_eq!(
        counters[1].load(Ordering::Relaxed),
        1,
        "Node2 (recipient) should receive the message"
    );
    assert_eq!(
        counters[2].load(Ordering::Relaxed),
        0,
        "Node3 should not receive the message"
    );
    assert_eq!(
        counters[3].load(Ordering::Relaxed),
        0,
        "Node4 should not receive the message"
    );

    for node in nodes {
        node.shutdown().await.ok();
    }
}
