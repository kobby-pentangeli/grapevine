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
    /// Port to listen on
    #[arg(short, long)]
    port: u16,

    /// Bootstrap peer addresses (can specify multiple)
    #[arg(short = 'b', long = "peer")]
    bootstrap_peers: Vec<SocketAddr>,

    /// Gossip interval in seconds
    #[arg(short, long, default_value = "5")]
    gossip_interval: u64,

    /// Fan-out factor
    #[arg(short, long, default_value = "3")]
    fanout: usize,

    /// Maximum number of peers
    #[arg(short, long, default_value = "50")]
    max_peers: usize,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize tracing
    let level = if args.debug {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let bind_addr = format!("127.0.0.1:{}", args.port).parse::<SocketAddr>()?;

    let mut builder = NodeConfigBuilder::new()
        .bind_addr(bind_addr)
        .gossip_interval(Duration::from_secs(args.gossip_interval))
        .fanout(args.fanout)
        .max_peers(args.max_peers);

    for peer in args.bootstrap_peers {
        builder = builder.add_bootstrap_peer(peer);
    }

    let config = builder.build()?;
    let node = Node::new(config).await?;
    node.on_message(|origin, data| {
        info!("Received from {origin}: {data:?}");
    })
    .await;
    node.start().await?;

    let local_addr = node.local_addr().await.expect("No local address");
    info!("Grapevine node started on {local_addr}");
    let node_clone = node.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            let message = format!("Heartbeat from {local_addr}");
            if let Err(e) = node_clone.broadcast(Bytes::from(message)).await {
                tracing::error!("Failed to broadcast: {e}");
            }
        }
    });

    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    node.shutdown().await?;
    info!("Goodbye!");

    Ok(())
}
