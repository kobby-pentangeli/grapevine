//! Grapevine interactive CLI application.

use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use clap::Parser;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use crossterm::{cursor, execute};
use grapevine::{Node, NodeConfigBuilder};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tracing::Level;
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

    /// Bootstrap peer address (can specify multiple)
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

/// Parse command from user input
enum Command {
    Broadcast(String),
    Send(SocketAddr, String),
    Peers,
    Status,
    Help,
    Quit,
    Unknown(String),
}

impl Command {
    fn parse(input: &str) -> Self {
        let input = input.trim();

        if input.is_empty() {
            return Command::Unknown(String::new());
        }

        if !input.starts_with('/') {
            return Command::Unknown(
                "Commands must start with '/'. Type /help for available commands.".to_string(),
            );
        }

        let parts = input.splitn(3, ' ').collect::<Vec<&str>>();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            "/broadcast" | "/b" => {
                if parts.len() < 2 {
                    return Command::Unknown("Usage: /broadcast <message>".to_string());
                }
                Command::Broadcast(parts[1..].join(" "))
            }
            "/send" | "/s" => {
                if parts.len() < 3 {
                    return Command::Unknown("Usage: /send <peer_address> <message>".to_string());
                }
                match parts[1].parse::<SocketAddr>() {
                    Ok(addr) => Command::Send(addr, parts[2].to_string()),
                    Err(_) => Command::Unknown(format!("Invalid peer address: {}", parts[1])),
                }
            }
            "/peers" | "/p" => Command::Peers,
            "/status" | "/st" => Command::Status,
            "/help" | "/h" | "/?" => Command::Help,
            "/quit" | "/exit" | "/q" => Command::Quit,
            _ => Command::Unknown(format!(
                "Unknown command: {cmd}. Type /help for available commands."
            )),
        }
    }
}

/// Print colored message
fn print_colored(color: Color, text: &str) {
    let mut stdout = io::stdout();
    execute!(stdout, SetForegroundColor(color), Print(text), ResetColor).ok();
    stdout.flush().ok();
}

/// Print colored message with a newline appended to the output
fn println_colored(color: Color, text: &str) {
    print_colored(color, text);
    println!();
}

/// Display welcome banner
fn display_banner() {
    println!();
    println_colored(
        Color::Cyan,
        "┌──────────────────────────────────────────────────────────┐",
    );
    println_colored(
        Color::Cyan,
        "│            Grapevine Interactive Gossip Client           │",
    );
    println_colored(
        Color::Cyan,
        "└──────────────────────────────────────────────────────────┘",
    );
    println!();
}

/// Display help message
fn display_help() {
    println!();
    println_colored(Color::Yellow, "Available Commands:");
    println!();
    println!("  /broadcast <message>  - Broadcast a message to all peers");
    println!("  /send <peer> <msg>    - Send a direct message to a specific peer");
    println!("  /peers                - List connected peers");
    println!("  /status               - Show node status");
    println!("  /help                 - Show this help message");
    println!("  /quit or /exit        - Exit gracefully");
    println!();
    println_colored(Color::DarkGrey, "Examples:");
    println_colored(Color::DarkGrey, "  /broadcast Hello everyone!");
    println_colored(Color::DarkGrey, "  /send 127.0.0.1:8001 Hi there");
    println_colored(Color::DarkGrey, "  /peers");
    println!();
}

/// Display prompt
fn display_prompt(local_addr: SocketAddr) {
    print_colored(Color::Green, &format!("grapevine@{local_addr}"));
    print_colored(Color::White, "> ");
    io::stdout().flush().ok();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load a local `.env` if present so its keys populate the process
    // environment that clap reads below; real environment variables already set
    // take precedence, so a missing file is fine.
    dotenvy::dotenv().ok();

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

    // Set up message handler with thread-safe output
    let output = Arc::new(Mutex::new(()));
    node.on_message(move |origin, data| {
        let output = Arc::clone(&output);
        tokio::spawn(async move {
            let _output_lock = output.lock().await;

            // Clear current line and move to beginning
            execute!(
                io::stdout(),
                cursor::MoveToColumn(0),
                Clear(ClearType::CurrentLine)
            )
            .ok();

            // Print received message
            print_colored(Color::Blue, "Message received from ");
            print_colored(Color::Cyan, &origin.to_string());
            print_colored(Color::Blue, ": ");

            // Try to decode as UTF-8, otherwise show hex
            match String::from_utf8(data.to_vec()) {
                Ok(s) => println_colored(Color::White, &s),
                Err(_) => println_colored(Color::DarkGrey, &format!("{data:?}")),
            }
        });
    })
    .await;

    node.start().await?;
    let local_addr = node
        .local_addr()
        .await
        .ok_or_else(|| io::Error::other("node has no local address after start"))?;

    display_banner();

    println_colored(Color::Green, &format!("Node started on {local_addr}"));
    println_colored(
        Color::White,
        &format!("  Gossip interval: {}s", args.gossip_interval),
    );
    println_colored(Color::White, &format!("  Fanout: {}", args.fanout));
    println_colored(Color::White, &format!("  Max peers: {}", args.max_peers));
    if !args.bootstrap_peers.is_empty() {
        println_colored(
            Color::White,
            &format!("  Bootstrap peers: {}", args.bootstrap_peers.len()),
        );
    }
    println!();
    println_colored(
        Color::Yellow,
        "Type /help for available commands or /quit to exit",
    );
    println!();

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    // Main command loop
    loop {
        display_prompt(local_addr);

        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                println_colored(Color::Red, &format!("Error reading input: {e}"));
                continue;
            }
        }

        let command = Command::parse(&line);

        match command {
            Command::Broadcast(msg) => match node.broadcast(Bytes::from(msg)).await {
                Ok(_) => {
                    println_colored(Color::Green, "Message broadcasted");
                }
                Err(e) => {
                    println_colored(Color::Red, &format!("Broadcast failed: {e}"));
                }
            },
            Command::Send(peer, msg) => match node.send_to_peer(peer, Bytes::from(msg)).await {
                Ok(_) => {
                    println_colored(Color::Green, &format!("Message sent to {peer}"));
                }
                Err(e) => {
                    println_colored(Color::Red, &format!("Send failed: {e}"));
                }
            },
            Command::Peers => {
                let peers = node.peers().await;
                if peers.is_empty() {
                    println_colored(Color::Yellow, "No connected peers");
                } else {
                    println_colored(Color::Cyan, &format!("Connected peers ({}):", peers.len()));
                    for peer in peers {
                        println_colored(Color::White, &format!("  -> {peer}"));
                    }
                }
            }
            Command::Status => {
                let peers = node.peers().await;
                let addr = node
                    .local_addr()
                    .await
                    .map_or_else(|| "N/A".to_string(), |a| a.to_string());

                println!();
                println_colored(Color::Cyan, "Node Status:");
                println_colored(Color::White, &format!("  Local address: {addr}"));
                println_colored(Color::White, &format!("  Connected peers: {}", peers.len()));
                println_colored(
                    Color::White,
                    &format!("  Gossip interval: {}s", args.gossip_interval),
                );
                println_colored(Color::White, &format!("  Fanout: {}", args.fanout));
                println!();
            }
            Command::Help => {
                display_help();
            }
            Command::Quit => {
                println!();
                node.shutdown().await?;
                println_colored(Color::Green, "Node shutdown complete. Goodbye!");
                println!();
                break;
            }
            Command::Unknown(msg) => {
                if !msg.is_empty() {
                    println_colored(Color::Red, &msg);
                }
            }
        }
    }

    Ok(())
}
