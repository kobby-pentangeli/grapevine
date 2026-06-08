//! Common test utilities shared across integration tests.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use grapevine::Node;

pub const READY_TIMEOUT: Duration = Duration::from_secs(10);

const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Initialize test tracing. Idempotent: subsequent calls are ignored.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
}

/// Poll `condition` until it holds or `timeout` elapses, panicking on timeout.
pub async fn wait_until(label: &str, timeout: Duration, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for: {label}");
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Wait until `node` reports at least `min` connected peers.
pub async fn wait_for_peers(node: &Node, min: usize, label: &str) {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if node.peers().await.len() >= min {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {min} peer(s): {label}"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Wait until `node`'s registry lists `addr` by its canonical listening address
/// (the precondition for `send_to_peer(addr)` to resolve a connection).
pub async fn wait_for_peer_addr(node: &Node, addr: SocketAddr, label: &str) {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if node.peers().await.contains(&addr) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for peer {addr}: {label}"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
