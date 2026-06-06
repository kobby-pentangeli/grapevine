//! Verify basic node operations including starting up,
//! shutting down gracefully, and configuration validation.

mod common;

use std::time::Duration;

use common::init_tracing;
use grapevine::{Node, NodeConfig, NodeConfigBuilder, RateLimitConfig};

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

/// Test graceful shutdown with connected peers.
#[tokio::test(flavor = "multi_thread")]
async fn graceful_shutdown_with_peers() {
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
    node2.start().await.expect("Failed to start node2");

    tokio::time::sleep(Duration::from_millis(200)).await;

    let shutdown_result = tokio::time::timeout(Duration::from_secs(2), node1.shutdown()).await;

    assert!(
        shutdown_result.is_ok(),
        "Shutdown with peers should complete within 2 seconds"
    );
    assert!(shutdown_result.unwrap().is_ok(), "Shutdown should succeed");

    node2.shutdown().await.ok();
}

/// Test configuration validation for given parameters.
#[tokio::test(flavor = "multi_thread")]
async fn validate_config_params() {
    init_tracing();

    // fanout > max_peers
    let maybe_config = NodeConfigBuilder::new().fanout(10).max_peers(5).build();
    assert!(maybe_config.is_err());
    match maybe_config {
        Err(grapevine::Error::Config(msg)) => {
            assert!(msg.contains("fanout cannot exceed max_peers"));
        }
        _ => panic!("Expected Config error"),
    }

    // gossip interval too short
    let maybe_config = NodeConfigBuilder::new()
        .gossip_interval(Duration::from_millis(500))
        .build();
    assert!(maybe_config.is_err());
    match maybe_config {
        Err(grapevine::Error::Config(msg)) => {
            assert!(msg.contains("gossip_interval must be >= 1 second"));
        }
        _ => panic!("Expected Config error"),
    }

    // gossip interval too long
    let maybe_config = NodeConfigBuilder::new()
        .gossip_interval(Duration::from_secs(7200))
        .build();
    assert!(maybe_config.is_err());
    match maybe_config {
        Err(grapevine::Error::Config(msg)) => {
            assert!(msg.contains("gossip_interval must be <= 1 hour"));
        }
        _ => panic!("Expected Config error"),
    }

    // peer timeout too short
    let maybe_config = NodeConfigBuilder::new()
        .peer_timeout(Duration::from_secs(2))
        .build();
    assert!(maybe_config.is_err());
    match maybe_config {
        Err(grapevine::Error::Config(msg)) => {
            assert!(msg.contains("peer_timeout must be >= 5 seconds"));
        }
        _ => panic!("Expected Config error"),
    }

    // rate limit capacity cannot be 0 when enabled
    let rate_limit = RateLimitConfig {
        enabled: true,
        capacity: 0,
        refill_rate: 10,
    };

    let maybe_config = NodeConfigBuilder::new().rate_limit(rate_limit).build();
    assert!(maybe_config.is_err());
    match maybe_config {
        Err(grapevine::Error::Config(msg)) => {
            assert!(msg.contains("Rate limit capacity must be greater than 0"));
        }
        _ => panic!("Expected Config error"),
    }
}
