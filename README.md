# Ananse
A simple peer-to-peer gossip protocol handler

## How to run

Using Cargo, pass the necessary command-line args after `--`, like the following:

```
cargo run -- --period 5 --port 800
```

## Usage

```
$ cargo run -- --help
    Finished dev [unoptimized + debuginfo] target(s) in 0.11s
     Running `target/debug/ananse -h`
ananse v0.1.0
Kobby Pentangeli <kobbypentangeli@gmail.com>
A simple peer-to-peer gossip protocol handler

USAGE:
    ananse [OPTIONS] --period <period> --port <port>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --connect <connect>    Sets the optional peer addr to connect to.
                                  Example: --connect 127.0.0.1:8000
        --period <period>      Sets the period (in seconds) of emitting messages to other peers.
                                  Example: --period 5
        --port <port>          Sets the port to listen to.
                                  Example: --port 8000
```
