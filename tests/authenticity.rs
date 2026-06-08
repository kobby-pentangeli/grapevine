//! Verify that cryptographic message authenticity holds end-to-end: a peer
//! cannot inject a message attributed to an origin whose key the receiver has
//! already pinned.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use common::init_tracing;
use grapevine::{Identity, Node, NodeConfigBuilder, Payload, Tcp};

/// Block until the recorded delivery set satisfies `predicate`, or fail.
async fn wait_for_delivery(
    label: &str,
    delivered: &Arc<Mutex<Vec<Bytes>>>,
    predicate: impl Fn(&[Bytes]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let satisfied = predicate(&delivered.lock().expect("record lock"));
        if satisfied {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// A node `B` pins origin `A` to `A`'s key the first time it sees an authentic
/// message from `A`. A forger who connects to `B` afterwards and sends a message
/// claiming `A`'s origin---but signed with the forger's own key---is
/// rejected, even though the forged message's signature is internally valid.
#[tokio::test(flavor = "multi_thread")]
async fn forged_origin_message_is_rejected() {
    init_tracing();

    let delivered: Arc<Mutex<Vec<Bytes>>> = Arc::new(Mutex::new(Vec::new()));

    // Victim B: records every application message it delivers.
    let recorder = Arc::clone(&delivered);
    let victim = Node::new(NodeConfigBuilder::new().build().expect("victim config"))
        .await
        .expect("create victim");
    victim
        .on_message(move |_origin, data| {
            recorder.lock().expect("record lock").push(data);
        })
        .await;
    victim.start().await.expect("start victim");
    let victim_addr = victim.local_addr().await.expect("victim address");

    // Honest A: bootstraps from B and broadcasts a legitimate message, which
    // pins A's origin to A's key at B.
    let honest = Node::new(
        NodeConfigBuilder::new()
            .add_bootstrap_peer(victim_addr)
            .build()
            .expect("honest config"),
    )
    .await
    .expect("create honest");
    honest.start().await.expect("start honest");
    let honest_addr = honest.local_addr().await.expect("honest address");

    let deadline = Instant::now() + Duration::from_secs(5);
    while honest.peers().await.is_empty() || victim.peers().await.is_empty() {
        assert!(Instant::now() < deadline, "A and B never connected");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    honest
        .broadcast(Bytes::from_static(b"legit-1"))
        .await
        .expect("broadcast legit-1");
    wait_for_delivery("B to deliver legit-1", &delivered, |records| {
        records.iter().any(|m| m == "legit-1")
    })
    .await;

    // Forger: a fresh transport connects to B and sends a message claiming A's
    // origin address but signed with the forger's own identity.
    let forger_identity = Identity::generate();
    let forged = forger_identity
        .author(
            honest_addr,
            999,
            Payload::Application(Bytes::from_static(b"forged")),
        )
        .expect("author forged message");

    let attacker = Tcp::new();
    attacker
        .connect(victim_addr)
        .await
        .expect("attacker connects");
    attacker
        .send(victim_addr, forged)
        .await
        .expect("attacker sends forged frame");

    // Barrier: a later legitimate broadcast from A is delivered, after which any
    // accepted forged message would also have been processed.
    honest
        .broadcast(Bytes::from_static(b"legit-2"))
        .await
        .expect("broadcast legit-2");
    wait_for_delivery("B to deliver legit-2", &delivered, |records| {
        records.iter().any(|m| m == "legit-2")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    attacker.shutdown().await;
    honest.shutdown().await.ok();
    victim.shutdown().await.ok();

    let records = delivered.lock().expect("record lock");
    assert!(
        records.iter().any(|m| m == "legit-1") && records.iter().any(|m| m == "legit-2"),
        "legitimate messages from the pinned origin are still delivered"
    );
    assert!(
        !records.iter().any(|m| m == "forged"),
        "a message spoofing a pinned origin must be rejected, got {records:?}"
    );
}
