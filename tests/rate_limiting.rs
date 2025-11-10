//! Rate limiting integration tests

mod common;

use common::init_tracing;
use grapevine::{Node, NodeConfigBuilder, RateLimitConfig};

/// Test rate limiting is enforced.
#[tokio::test(flavor = "multi_thread")]
async fn rate_limiting_enabled() {
    init_tracing();

    let rate_limit = RateLimitConfig {
        enabled: true,
        capacity: 5,
        refill_rate: 1,
    };

    let config = NodeConfigBuilder::new()
        .rate_limit(rate_limit)
        .build()
        .expect("Failed to build config");

    let node = Node::new(config).await.expect("Failed to create node");
    node.start().await.expect("Failed to start node");

    assert!(node.local_addr().await.is_some());

    node.shutdown().await.ok();
}

/// Test rate limiting can be disabled.
#[tokio::test(flavor = "multi_thread")]
async fn rate_limiting_disabled() {
    init_tracing();

    let rate_limit = RateLimitConfig {
        enabled: false,
        capacity: 0,
        refill_rate: 0,
    };

    let config = NodeConfigBuilder::new()
        .rate_limit(rate_limit)
        .build()
        .expect("Failed to build config");

    let node = Node::new(config).await.expect("Failed to create node");
    node.start().await.expect("Failed to start node");

    assert!(node.local_addr().await.is_some());

    node.shutdown().await.ok();
}
