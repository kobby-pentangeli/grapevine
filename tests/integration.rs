//! Basic integration tests for Grapevine.
//!
//! These tests verify basic functionality without requiring long-running clusters.

use std::time::Duration;

use grapevine::{Node, NodeConfig, NodeConfigBuilder};

/// Initialize test tracing (call once at the beginning of tests).
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
}

/// Test that a node can start up successfully.
#[tokio::test(flavor = "multi_thread")]
async fn node_startup() {
    init_tracing();

    let config = NodeConfig::default();
    let node = Node::new(config).await.expect("Failed to create node");
    node.start().await.expect("Failed to start node");

    let addr = node.local_addr().await;
    assert!(
        addr.is_some(),
        "Node should have a local address after starting"
    );
    assert!(
        addr.unwrap().port() > 0,
        "Node should be listening on a valid port"
    );

    // Cleanup
    node.shutdown().await.ok();
}

/// Test node startup with custom configuration.
#[tokio::test(flavor = "multi_thread")]
async fn node_startup_custom_config() {
    init_tracing();

    let config = NodeConfigBuilder::new()
        .gossip_interval(Duration::from_secs(1))
        .fanout(5)
        .max_peers(100)
        .peer_timeout(Duration::from_secs(60))
        .build()
        .expect("Failed to build config");

    let node = Node::new(config.clone())
        .await
        .expect("Failed to create node");
    node.start().await.expect("Failed to start node");

    assert!(node.local_addr().await.is_some());
    assert_eq!(node.config.fanout, 5);
    assert_eq!(node.config.max_peers, 100);

    node.shutdown().await.ok();
}

/// Test node behavior with ephemeral port (bind to port 0).
#[tokio::test(flavor = "multi_thread")]
async fn ephemeral_port() {
    init_tracing();

    let config = NodeConfig::default(); // Should bind to 127.0.0.1:0
    let node = Node::new(config).await.expect("Failed to create node");
    node.start().await.expect("Failed to start node");

    let addr = node.local_addr().await.expect("No local address");
    assert!(addr.port() > 0, "Expected OS to assign a port, got port 0");

    node.shutdown().await.ok();
}

/// Test that node returns correct local address after starting.
#[tokio::test(flavor = "multi_thread")]
async fn local_address_consistency() {
    init_tracing();

    let node = Node::new(NodeConfig::default())
        .await
        .expect("Failed to create node");
    node.start().await.expect("Failed to start node");

    let addr1 = node.local_addr().await.expect("No local address");

    for _ in 0..5 {
        let addr = node.local_addr().await.expect("No local address");
        assert_eq!(
            addr, addr1,
            "Local address should remain consistent after startup"
        );
    }

    node.shutdown().await.ok();
}

/// Test that shutdown works correctly for all background tasks.
#[tokio::test(flavor = "multi_thread")]
async fn node_shutdown() {
    init_tracing();

    let config = NodeConfig::default();
    let node = Node::new(config).await.expect("Failed to create node");
    node.start().await.expect("Failed to start node");

    // Verify node is running
    assert!(node.local_addr().await.is_some());

    // Shutdown should complete quickly
    let shutdown_result = tokio::time::timeout(Duration::from_secs(1), node.shutdown()).await;

    assert!(
        shutdown_result.is_ok(),
        "Shutdown should complete within 1 second"
    );
    assert!(shutdown_result.unwrap().is_ok(), "Shutdown should succeed");
}
