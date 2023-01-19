use clap::{arg, value_parser, Command};
use grapevine::{logger, node::Node};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new("Grapevine")
        .version("0.1.0")
        .author("Kobby Pentangeli <kobbypentangeli@gmail.com>")
        .about("A simple peer-to-peer gossip protocol handler")
        .args(&[
            arg!(-p --port <PORT> "Sets the port to listen to.\n   Example: --port 8000")
                .value_parser(value_parser!(u32))
                .required(true),
            arg!(-d --duration <DURATION> "Sets the duration (in seconds) of emitting messages to other peers.\n   Example: --duration 5")
                .value_parser(value_parser!(u32))
                .required(true),
            arg!(--peer <PEER> "Sets the optional peer address to connect to.\n   Example: --peer 127.0.0.1:8000")
                .value_parser(value_parser!(String))
                .required(false),
        ])
        .get_matches();

    let port = matches.get_one::<u32>("port").expect("Port not specified");
    let duration = matches
        .get_one::<u32>("duration")
        .expect("Duration not specified");
    let peer = matches.get_one::<String>("peer");

    logger::init()?;

    Node::new(*port, *duration, peer.cloned())?.execute()?;
    Ok(())
}
