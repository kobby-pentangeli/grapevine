//! Verify that a message the epidemic push fails to deliver is
//! still reconciled through the digest exchange.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use common::init_tracing;
use grapevine::{AntiEntropyConfig, EpidemicConfig, Node, NodeConfigBuilder};

fn no_forwarding() -> EpidemicConfig {
    EpidemicConfig {
        forward_probability: 0.0,
    }
}

/// Frequent reconciliation so the test observes convergence quickly.
fn brisk_anti_entropy() -> AntiEntropyConfig {
    AntiEntropyConfig {
        interval: Duration::from_millis(500),
        fanout: 3,
        enabled: true,
    }
}

/// Topology `A -> B <- C`, where `max_peers = 1` on the leaves keeps transitive
/// discovery from completing the `A <-> C` edge. With forwarding disabled, `B`
/// receives `A`'s broadcast but never relays it; `C` can only learn the message
/// through `B`'s anti-entropy digest. The test fails (times out) if the digest
/// exchange does not repair `C`.
#[tokio::test(flavor = "multi_thread")]
async fn message_missed_by_epidemic_is_recovered_by_anti_entropy() {
    init_tracing();

    // Hub B: holds both leaves, never forwards.
    let hub_config = NodeConfigBuilder::new()
        .max_peers(8)
        .epidemic(no_forwarding())
        .anti_entropy(brisk_anti_entropy())
        .build()
        .expect("Failed to build hub config");
    let hub = Node::new(hub_config).await.expect("Failed to create hub");
    hub.start().await.expect("Failed to start hub");
    let hub_addr = hub.local_addr().await.expect("No hub address");

    // Broadcaster A: capped at a single peer so it never dials C.
    let sender_config = NodeConfigBuilder::new()
        .add_bootstrap_peer(hub_addr)
        .max_peers(1)
        .fanout(1)
        .epidemic(no_forwarding())
        .anti_entropy(brisk_anti_entropy())
        .build()
        .expect("Failed to build sender config");
    let sender = Node::new(sender_config)
        .await
        .expect("Failed to create sender");
    sender.start().await.expect("Failed to start sender");

    // Receiver C: also capped at one peer; must converge via anti-entropy only.
    let received = Arc::new(AtomicBool::new(false));
    let received_clone = Arc::clone(&received);

    let receiver_config = NodeConfigBuilder::new()
        .add_bootstrap_peer(hub_addr)
        .max_peers(1)
        .fanout(1)
        .epidemic(no_forwarding())
        .anti_entropy(brisk_anti_entropy())
        .build()
        .expect("Failed to build receiver config");
    let receiver = Node::new(receiver_config)
        .await
        .expect("Failed to create receiver");
    receiver
        .on_message(move |_origin, data| {
            if data == "anti-entropy please" {
                received_clone.store(true, Ordering::SeqCst);
            }
        })
        .await;
    receiver.start().await.expect("Failed to start receiver");

    // Wait until the line topology is established: both leaves attached to the
    // hub, and the hub holding exactly the two leaves.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let hub_peers = hub.peers().await.len();
        let sender_peers = sender.peers().await.len();
        let receiver_peers = receiver.peers().await.len();
        if hub_peers == 2 && sender_peers == 1 && receiver_peers == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "topology never settled (hub={hub_peers}, sender={sender_peers}, receiver={receiver_peers})"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    sender
        .broadcast(Bytes::from("anti-entropy please"))
        .await
        .expect("Failed to broadcast");

    // C never received the push (forwarding is off); only the digest exchange
    // can deliver it. Allow several anti-entropy rounds.
    let deadline = Instant::now() + Duration::from_secs(15);
    while !received.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "receiver never converged via anti-entropy"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    sender.shutdown().await.ok();
    receiver.shutdown().await.ok();
    hub.shutdown().await.ok();
}
