//! Performance benchmarks for the Grapevine gossip protocol.
//!
//! These benchmarks measure:
//! - Message codec performance (encoding/decoding)
//! - Message creation overhead
//! - Different payload type performance
//! - End-to-end dissemination latency versus network size and versus fanout

use std::hint::black_box;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use grapevine::{
    AntiEntropyConfig, EpidemicConfig, Message, MessageCodec, Node, NodeConfigBuilder, Payload,
};
use tokio::runtime::Runtime;
use tokio_util::codec::{Decoder, Encoder};

/// Benchmark message codec encoding performance.
fn message_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_encoding");

    let addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let mut codec = MessageCodec::new();

    // Benchmark different message sizes
    for size in [100, 1024, 10_000, 100_000].iter() {
        let data = Bytes::from(vec![0u8; *size]);
        let message = Message::new(addr, 0, Payload::Application(data));

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let mut buffer = bytes::BytesMut::new();
                codec
                    .encode(black_box(message.clone()), &mut buffer)
                    .unwrap();
                black_box(buffer);
            });
        });
    }

    group.finish();
}

/// Benchmark message codec decoding performance.
fn message_decoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_decoding");

    let addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let mut codec = MessageCodec::new();

    // Pre-encode messages of different sizes
    for size in [100, 1024, 10_000, 100_000].iter() {
        let data = Bytes::from(vec![0u8; *size]);
        let message = Message::new(addr, 0, Payload::Application(data));

        let mut buffer = bytes::BytesMut::new();
        codec.encode(message, &mut buffer).unwrap();

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let mut buf = buffer.clone();
                let decoded = codec.decode(&mut buf).unwrap();
                black_box(decoded);
            });
        });
    }

    group.finish();
}

/// Benchmark message creation.
fn message_creation(c: &mut Criterion) {
    let addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();

    c.bench_function("message_creation", |b| {
        b.iter(|| {
            let data = Bytes::from("test message");
            let message = Message::new(black_box(addr), 0, Payload::Application(data));
            black_box(message);
        });
    });
}

/// Benchmark different payload types.
fn payload_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("payload_encoding");
    let addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let mut codec = MessageCodec::new();

    // PeerListRequest
    group.bench_function("peer_list_request", |b| {
        let message = Message::new(addr, 0, Payload::PeerListRequest);
        b.iter(|| {
            let mut buffer = bytes::BytesMut::new();
            codec
                .encode(black_box(message.clone()), &mut buffer)
                .unwrap();
            black_box(buffer);
        });
    });

    // Heartbeat
    group.bench_function("heartbeat", |b| {
        let message = Message::new(addr, 0, Payload::Heartbeat { from: addr });
        b.iter(|| {
            let mut buffer = bytes::BytesMut::new();
            codec
                .encode(black_box(message.clone()), &mut buffer)
                .unwrap();
            black_box(buffer);
        });
    });

    // Application data
    group.bench_function("application", |b| {
        let data = Bytes::from("application payload");
        let message = Message::new(addr, 0, Payload::Application(data));
        b.iter(|| {
            let mut buffer = bytes::BytesMut::new();
            codec
                .encode(black_box(message.clone()), &mut buffer)
                .unwrap();
            black_box(buffer);
        });
    });

    group.finish();
}

fn send_path_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("send_path_serialization");

    let addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();
    let message = Message::new(addr, 0, Payload::Application(Bytes::from(vec![0u8; 1024])));
    let config = bincode::config::standard();

    group.bench_function("old_encode_decode_encode", |b| {
        b.iter(|| {
            let bytes = bincode::serde::encode_to_vec(black_box(&message), config).unwrap();
            let (decoded, _): (Message, _) =
                bincode::serde::decode_from_slice(&bytes, config).unwrap();
            let reencoded = bincode::serde::encode_to_vec(&decoded, config).unwrap();
            black_box(reencoded);
        });
    });

    group.bench_function("new_single_encode", |b| {
        b.iter(|| {
            let bytes = bincode::serde::encode_to_vec(black_box(&message), config).unwrap();
            black_box(bytes);
        });
    });

    group.finish();
}

/// Convergence ceiling for the dissemination benchmarks. Reached only on
/// failure; a healthy broadcast converges far sooner (the wait returns as soon
/// as every leaf has delivered the new message).
const CONVERGE_DEADLINE: Duration = Duration::from_secs(20);

fn bench_runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build benchmark runtime")
}

// Full forwarding plus brisk reconciliation, so a broadcast always converges to
// the whole cluster and the measured time is the dissemination latency rather
// than a probabilistic loss artefact.
fn flood() -> EpidemicConfig {
    EpidemicConfig {
        forward_probability: 1.0,
    }
}
fn brisk_anti_entropy() -> AntiEntropyConfig {
    AntiEntropyConfig {
        interval: Duration::from_millis(200),
        fanout: 4,
        enabled: true,
    }
}

/// Build a star cluster: `nodes[0]` is the hub/origin and the rest bootstrap
/// from it. Each leaf increments its counter on every application message.
/// Returns once the hub holds every leaf. `counters[0]` is a placeholder so the
/// counter and node indices line up.
async fn build_star(size: usize, fanout: usize) -> (Vec<Node>, Vec<Arc<AtomicU32>>) {
    let hub = Node::new(
        NodeConfigBuilder::new()
            .fanout(fanout)
            .epidemic(flood())
            .anti_entropy(brisk_anti_entropy())
            .build()
            .expect("hub config"),
    )
    .await
    .expect("create hub");
    hub.start().await.expect("start hub");
    let hub_addr = hub.local_addr().await.expect("hub address");

    let mut nodes = vec![hub];
    let mut counters = vec![Arc::new(AtomicU32::new(0))];

    for _ in 1..size {
        let node = Node::new(
            NodeConfigBuilder::new()
                .add_bootstrap_peer(hub_addr)
                .fanout(fanout)
                .epidemic(flood())
                .anti_entropy(brisk_anti_entropy())
                .build()
                .expect("leaf config"),
        )
        .await
        .expect("create leaf");

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);
        node.on_message(move |_origin, _data| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        })
        .await;
        node.start().await.expect("start leaf");

        nodes.push(node);
        counters.push(counter);
    }

    let deadline = Instant::now() + CONVERGE_DEADLINE;
    while nodes[0].peers().await.len() < size - 1 {
        assert!(Instant::now() < deadline, "star topology never formed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    (nodes, counters)
}

/// Broadcast one message from the hub and return the time until every leaf has
/// delivered it (one more delivery than its pre-broadcast baseline).
async fn broadcast_and_await(nodes: &[Node], counters: &[Arc<AtomicU32>]) -> Duration {
    let baseline = counters[1..]
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .collect::<Vec<u32>>();

    let start = Instant::now();
    nodes[0]
        .broadcast(Bytes::from_static(b"bench"))
        .await
        .expect("broadcast");

    let deadline = start + CONVERGE_DEADLINE;
    loop {
        let converged = counters[1..]
            .iter()
            .zip(&baseline)
            .all(|(c, base)| c.load(Ordering::Relaxed) > *base);
        if converged {
            break;
        }
        assert!(Instant::now() < deadline, "broadcast never fully delivered");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    start.elapsed()
}

async fn shutdown_all(nodes: Vec<Node>) {
    for node in nodes {
        node.shutdown().await.ok();
    }
}

/// Dissemination latency as the cluster grows (fixed fanout).
fn propagation_latency_vs_size(c: &mut Criterion) {
    let rt = bench_runtime();
    let mut group = c.benchmark_group("propagation_latency_vs_size");

    for &size in &[3usize, 5, 8] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.to_async(&rt).iter_custom(|iters| async move {
                let (nodes, counters) = build_star(size, 3).await;
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    total += broadcast_and_await(&nodes, &counters).await;
                }
                shutdown_all(nodes).await;
                total
            });
        });
    }

    group.finish();
}

/// Dissemination latency as fanout varies (fixed eight-node cluster): a low
/// fanout leaves more nodes to be repaired by anti-entropy, raising latency.
fn propagation_latency_vs_fanout(c: &mut Criterion) {
    let rt = bench_runtime();
    let mut group = c.benchmark_group("propagation_latency_vs_fanout");

    for &fanout in &[1usize, 2, 4] {
        group.bench_with_input(
            BenchmarkId::from_parameter(fanout),
            &fanout,
            |b, &fanout| {
                b.to_async(&rt).iter_custom(|iters| async move {
                    let (nodes, counters) = build_star(8, fanout).await;
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        total += broadcast_and_await(&nodes, &counters).await;
                    }
                    shutdown_all(nodes).await;
                    total
                });
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = codec_benches;
    config = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(3));
    targets =
        message_encoding,
        message_decoding,
        message_creation,
        payload_types,
        send_path_serialization
}

criterion_group! {
    name = dissemination_benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(10));
    targets =
        propagation_latency_vs_size,
        propagation_latency_vs_fanout
}

criterion_main!(codec_benches, dissemination_benches);
