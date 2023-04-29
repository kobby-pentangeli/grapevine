# Grapevine

A simple peer-to-peer gossip protocol handler

## How to run

Using Cargo, pass the necessary command-line args after `--`, like the following:

```rust
cargo run -- --duration 5 --port 8000 --peer 127.0.0.1:8000
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
  -p, --port <PORT>          Sets the port to listen to.
                                Example: --port 8000
  -d, --duration <DURATION>  Sets the duration (in seconds) of emitting messages to other peers.
                                Example: --duration 5
      --peer <PEER>          Sets the optional peer address to connect to.
                                Example: --peer 127.0.0.1:8000
  -h, --help                 Print help
  -V, --version              Print version
```

## Contributing

Thank you for considering to contribute to this project!

All contributions large and small are actively accepted.

- To get started, please read the [contribution guidelines](https://github.com/kobby-pentangeli/grapevine/blob/master/CONTRIBUTING.md).

- Browse [Good First Issues](https://github.com/kobby-pentangeli/grapevine/labels/good%20first%20issue).

## License

Licensed under either of <a href="LICENSE-APACHE">Apache License, Version 2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this codebase by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
