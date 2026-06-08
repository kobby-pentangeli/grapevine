//! Rate limiting integration tests.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use common::{READY_TIMEOUT, init_tracing, wait_for_peers};
use grapevine::{Node, NodeConfigBuilder, RateLimitConfig};

/// Test rate limiting is enabled and the node starts with a limiter configured.
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

#[tokio::test(flavor = "multi_thread")]
async fn rate_limited_peer_drops_excess_inbound() {
    init_tracing();

    const BURST: u32 = 60;

    // Receiver B: a tight limiter and a counter of delivered application messages.
    let receiver = Node::new(
        NodeConfigBuilder::new()
            .rate_limit(RateLimitConfig {
                enabled: true,
                capacity: 5,
                refill_rate: 1,
            })
            .build()
            .expect("Failed to build receiver config"),
    )
    .await
    .expect("Failed to create receiver");

    let delivered = Arc::new(AtomicU32::new(0));
    let delivered_clone = Arc::clone(&delivered);
    receiver
        .on_message(move |_origin, _data| {
            delivered_clone.fetch_add(1, Ordering::Relaxed);
        })
        .await;
    receiver.start().await.expect("Failed to start receiver");
    let receiver_addr = receiver.local_addr().await.expect("No receiver address");

    // Sender A: bootstraps from B so its frames arrive on B's rate-limited path.
    let sender = Node::new(
        NodeConfigBuilder::new()
            .add_bootstrap_peer(receiver_addr)
            .build()
            .expect("Failed to build sender config"),
    )
    .await
    .expect("Failed to create sender");
    sender.start().await.expect("Failed to start sender");

    wait_for_peers(&sender, 1, "sender connects to receiver").await;

    for i in 0..BURST {
        sender
            .broadcast(Bytes::from(format!("burst {i}")))
            .await
            .expect("Failed to broadcast");
    }

    let mut last = 0;
    let mut stable_since = Instant::now();
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let count = delivered.load(Ordering::Relaxed);
        if count != last {
            last = count;
            stable_since = Instant::now();
        }
        if count >= 1 && stable_since.elapsed() >= Duration::from_millis(500) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "rate-limited delivery never started or never quiesced (delivered={count})"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        last >= 1,
        "at least one message should pass before the bucket empties"
    );
    assert!(
        last < BURST / 2,
        "the limiter must drop most of the burst, delivered {last} of {BURST}"
    );

    sender.shutdown().await.ok();
    receiver.shutdown().await.ok();
}
