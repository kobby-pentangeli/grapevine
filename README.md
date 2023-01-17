# Grapevine

A simple peer-to-peer gossip protocol handler

## How to run

Using Cargo, pass the necessary command-line args after `--`, like the following:

```rust
cargo run -- --duration 5 --port 8000
```

## Usage

```rust
$ cargo run -- --help
    Finished dev [unoptimized + debuginfo] target(s) in 0.11s
     Running `target/debug/grapevine -h`
grapevine v0.1.0
Kobby Pentangeli <kobbypentangeli@gmail.com>
A simple peer-to-peer gossip protocol handler

Usage: main [OPTIONS] --port <PORT> --duration <DURATION>

Options:
  -p, --port <PORT>              Sets the port to listen to.
                                    Example: --port 8000
  -d, --duration <DURATION>      Sets the duration (in seconds) of emitting messages to other peers.
                                    Example: --duration 5
  -c, --connection <CONNECTION>  Sets the optional peer address to connect to.
                                    Example: --connection 127.0.0.1:8000
  -h, --help                     Print help
  -V, --version                  Print version
```
