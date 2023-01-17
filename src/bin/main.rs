use clap::{value_t, App, Arg};
use grapevine::{logger, node};
use node::Node;

fn main() {
    let arg_matches = App::new("Grapevine")
        .version("0.1.0")
        .author("Kobby Pentangeli <kobbypentangeli@gmail.com>")
        .about("A simple peer-to-peer gossip protocol handler")
        .arg(
            Arg::with_name("port")
                .long("port")
                .long_help("Sets the port to listen to.\n   Example: --port 8000")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("duration")
                .long("duration")
                .long_help("Sets the duration (in seconds) of emitting messages to other peers.\n   Example: --duration 5")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("connection")
                .long("connection")
                .long_help("Sets the optional peer address to connect to.\n   Example: --connection 127.0.0.1:8000")
                .takes_value(true),
        )
        .get_matches();

    let port = value_t!(arg_matches, "port", u32).unwrap();
    let duration = value_t!(arg_matches, "duration", u32).unwrap();
    let connection = value_t!(arg_matches, "connection", String).ok();

    logger::init();

    Node::new(port, duration, connection).unwrap().execute();
}
