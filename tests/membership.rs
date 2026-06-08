//! Verify that the peer registry is real: the documented `max_peers` bound is
//! enforced at connection time, and a peer that leaves is dropped from the
//! registry.

mod common;

use std::time::{Duration, Instant};

use common::init_tracing;
use grapevine::{Node, NodeConfig, NodeConfigBuilder};

/// A hub configured with `max_peers = 2` must never hold more than two peers,
/// even when more nodes try to connect.
#[tokio::test(flavor = "multi_thread")]
async fn refuses_connections_past_max_peers() {
    init_tracing();

    let hub_config = NodeConfigBuilder::new()
        .max_peers(2)
        .fanout(1)
        .build()
        .expect("Failed to build hub config");
    let hub = Node::new(hub_config).await.expect("Failed to create hub");
    hub.start().await.expect("Failed to start hub");
    let hub_addr = hub.local_addr().await.expect("No local address");

    let mut clients = Vec::new();
    for _ in 0..3 {
        let config = NodeConfigBuilder::new()
            .add_bootstrap_peer(hub_addr)
            .build()
            .expect("Failed to build client config");
        let client = Node::new(config).await.expect("Failed to create client");
        client.start().await.expect("Failed to start client");
        clients.push(client);
    }

    // The hub admits up to its cap.
    let deadline = Instant::now() + Duration::from_secs(5);
    while hub.peers().await.len() < 2 {
        assert!(Instant::now() < deadline, "hub never reached its peer cap");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // It refuses the rest: give any extra/refused connections time to settle,
    // then confirm the bound still holds.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        hub.peers().await.len(),
        2,
        "hub must not exceed max_peers (2)"
    );

    hub.shutdown().await.ok();
    for client in clients {
        client.shutdown().await.ok();
    }
}

/// When a peer shuts down, the other node must drop it from its registry.
#[tokio::test(flavor = "multi_thread")]
async fn peer_removed_on_disconnect() {
    init_tracing();

    let node_a = Node::new(NodeConfig::default())
        .await
        .expect("Failed to create node A");
    node_a.start().await.expect("Failed to start node A");
    let addr_a = node_a.local_addr().await.expect("No local address");

    let config_b = NodeConfigBuilder::new()
        .add_bootstrap_peer(addr_a)
        .build()
        .expect("Failed to build node B config");
    let node_b = Node::new(config_b).await.expect("Failed to create node B");
    node_b.start().await.expect("Failed to start node B");
    let addr_b = node_b.local_addr().await.expect("No local address");

    // A learns B by its canonical listening address.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !node_a.peers().await.contains(&addr_b) {
        assert!(Instant::now() < deadline, "A never registered B");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    node_b.shutdown().await.ok();

    // After B leaves, A's registry no longer lists it.
    let deadline = Instant::now() + Duration::from_secs(5);
    while node_a.peers().await.contains(&addr_b) {
        assert!(
            Instant::now() < deadline,
            "A never dropped B after disconnect"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    node_a.shutdown().await.ok();
}
