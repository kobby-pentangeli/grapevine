//! Performance benchmarks for the Grapevine gossip protocol.
//!
//! These benchmarks measure:
//! - Message codec performance (encoding/decoding)
//! - Message creation overhead
//! - Different payload type performance

use std::hint::black_box;
use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use grapevine::{Message, MessageCodec, Payload};
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

criterion_group! {
    name = benches;
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

criterion_main!(benches);
