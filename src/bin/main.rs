//! Grapevine CLI application.

use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use clap::Parser;
use grapevine::{Node, NodeConfigBuilder};
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(name = "grapevine")]
#[command(about = "A modern, asynchronous peer-to-peer gossip protocol client", long_about = None)]
#[command(version)]
struct Args {
    /// Host to bind to
    #[arg(short = 'H', long, env = "BIND_HOST", default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on
    #[arg(short, long, env = "BIND_PORT", default_value = "8000")]
    port: u16,

    /// Bootstrap peer addresses (can specify multiple)
    #[arg(
        short = 'b',
        long = "peer",
        env = "BOOTSTRAP_PEERS",
        value_delimiter = ','
    )]
    bootstrap_peers: Vec<SocketAddr>,

    /// Gossip interval in seconds
    #[arg(short, long, env = "GOSSIP_INTERVAL_SECS", default_value = "5")]
    gossip_interval: u64,

    /// Fan-out factor
    #[arg(short, long, env = "FANOUT", default_value = "3")]
    fanout: usize,

    /// Maximum number of peers
    #[arg(short, long, env = "MAX_PEERS", default_value = "50")]
    max_peers: usize,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short = 'l', long, env = "RUST_LOG", default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize tracing
    let level = match args.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => {
            eprintln!("Invalid log level '{}', using 'info'", args.log_level);
            Level::INFO
        }
    };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let bind_addr = format!("{}:{}", args.host, args.port).parse::<SocketAddr>()?;

    let mut builder = NodeConfigBuilder::new()
        .bind_addr(bind_addr)
        .gossip_interval(Duration::from_secs(args.gossip_interval))
        .fanout(args.fanout)
        .max_peers(args.max_peers);

    for peer in &args.bootstrap_peers {
        builder = builder.add_bootstrap_peer(*peer);
    }

    let config = builder.build()?;
    let node = Node::new(config).await?;
    node.on_message(|origin, data| {
        info!("Received from {origin}: {data:?}");
    })
    .await;
    node.start().await?;

    let local_addr = node.local_addr().await.expect("No local address");

    println!();
    println!("┌─────────────────────────────────────────┐");
    println!("│   Grapevine Node Started Successfully   │");
    println!("└─────────────────────────────────────────┘");
    println!("  Listening on: {}", local_addr);
    println!("  Gossip interval: {}s", args.gossip_interval);
    println!("  Fanout: {}", args.fanout);
    println!("  Max peers: {}", args.max_peers);
    if !args.bootstrap_peers.is_empty() {
        println!("  Bootstrap peers: {}", args.bootstrap_peers.len());
    }
    println!();
    println!("Press Ctrl+C to shutdown");
    println!();

    info!("Node initialized and ready");

    let node_clone = node.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            let peers = node_clone.peers().await;
            if !peers.is_empty() {
                info!("Connected to {} peer(s)", peers.len());
            }
            let message = format!("Heartbeat from {local_addr}");
            if let Err(e) = node_clone.broadcast(Bytes::from(message)).await {
                tracing::error!("Failed to broadcast: {e}");
            }
        }
    });

    tokio::signal::ctrl_c().await?;
    println!();
    info!("Received shutdown signal, gracefully shutting down...");
    node.shutdown().await?;
    info!("Node shutdown complete. Goodbye!");

    Ok(())
}
