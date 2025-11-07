# Grapevine CLI Usage Guide

This guide provides step-by-step instructions for using the Grapevine CLI client to create and manage gossip networks.

## Table of Contents

- [Grapevine CLI Usage Guide](#grapevine-cli-usage-guide)
  - [Table of Contents](#table-of-contents)
  - [Installation](#installation)
  - [Quick Start](#quick-start)
  - [Starting a Node](#starting-a-node)
    - [Starting the First Node (Seed Node)](#starting-the-first-node-seed-node)
    - [Joining an Existing Network](#joining-an-existing-network)
    - [Starting with Environment Variables](#starting-with-environment-variables)
  - [Network Operations](#network-operations)
    - [Broadcasting Messages](#broadcasting-messages)
    - [Monitoring Network Activity](#monitoring-network-activity)
    - [Viewing Connected Peers](#viewing-connected-peers)
  - [Leaving the Network](#leaving-the-network)
  - [Advanced Usage](#advanced-usage)
    - [Custom Configuration](#custom-configuration)
    - [Multi-Node Local Cluster](#multi-node-local-cluster)
    - [Tuning Network Parameters](#tuning-network-parameters)
  - [Limitations](#limitations)
    - [Current CLI Limitations](#current-cli-limitations)
    - [Protocol Limitations](#protocol-limitations)
  - [Troubleshooting](#troubleshooting)
    - [Node won't start](#node-wont-start)
    - [Can't connect to peers](#cant-connect-to-peers)
    - [Not receiving messages](#not-receiving-messages)
    - [High CPU or bandwidth usage](#high-cpu-or-bandwidth-usage)
  - [Next Steps](#next-steps)
  - [Support](#support)

## Installation

Build the Grapevine CLI from source:

```bash
# Clone the repository
git clone https://github.com/kobby-pentangeli/grapevine
cd grapevine

# Build the binary
cargo build --release

# The binary will be at: target/release/grapevine
```

## Quick Start

The fastest way to get started is to run a single node:

```bash
# Start a node on the default port (8000)
cargo run --bin grapevine
```

You'll see output like:

```bash
┌─────────────────────────────────────────┐
│   Grapevine Node Started Successfully   │
└─────────────────────────────────────────┘
  Listening on: 127.0.0.1:8000
  Gossip interval: 5s
  Fanout: 3
  Max peers: 50

Press Ctrl+C to shutdown
```

## Starting a Node

### Starting the First Node (Seed Node)

The first node in a network acts as the **seed node** that other nodes will connect to:

```bash
# Start seed node on default port (8000)
cargo run --bin grapevine

# Or specify custom host and port
cargo run --bin grapevine -- --host 0.0.0.0 --port 9000
```

The seed node will:

- Listen for incoming peer connections
- Broadcast periodic heartbeat messages
- Log received messages from other nodes

### Joining an Existing Network

To join an existing network, specify one or more **bootstrap peers**:

```bash
# Join network with a single bootstrap peer
cargo run --bin grapevine -- --port 8001 --peer 127.0.0.1:8000

# Join with multiple bootstrap peers (comma-separated)
cargo run --bin grapevine -- --port 8002 \
  --peer 127.0.0.1:8000,127.0.0.1:8001
```

The node will:

1. Connect to the specified bootstrap peer(s)
2. Request their peer lists
3. Discover and connect to other nodes in the network
4. Begin participating in gossip protocol

### Starting with Environment Variables

For easier deployment, use environment variables:

```bash
# Create a .env file
cat > .env <<EOF
BIND_HOST=0.0.0.0
BIND_PORT=8000
BOOTSTRAP_PEERS=127.0.0.1:8001,127.0.0.1:8002
GOSSIP_INTERVAL_SECS=5
FANOUT=3
MAX_PEERS=50
RUST_LOG=info
EOF

# Start node (automatically reads .env)
cargo run --bin grapevine
```

## Network Operations

### Broadcasting Messages

The CLI node automatically broadcasts heartbeat messages every 10 seconds:

```bash
Heartbeat from 127.0.0.1:8000
```

These heartbeats demonstrate the gossip protocol in action. In the logs, you'll see messages received from other nodes:

```bash
2025-11-04T10:00:00.000000Z  INFO grapevine: Received from 127.0.0.1:8001: b"Heartbeat from 127.0.0.1:8001"
```

**Note**: The current CLI is designed for demonstration. To broadcast custom messages, you'll need to:

1. Use the Grapevine library API in your own application
2. Or modify the CLI source code to accept message input

### Monitoring Network Activity

Control log verbosity to monitor different aspects of the network:

```bash
# Info level (default) - shows received messages and connection status
cargo run --bin grapevine -- --log-level info

# Debug level - shows protocol details and peer discovery
cargo run --bin grapevine -- --log-level debug

# Trace level - shows all protocol internals
cargo run --bin grapevine -- --log-level trace
```

Example debug output:

```bash
2025-11-04T10:00:00.000000Z  INFO grapevine::transport::tcp: Connected to peer 127.0.0.1:8001
2025-11-04T10:00:05.000000Z  INFO grapevine: Connected to 3 peer(s)
2025-11-04T10:00:10.000000Z DEBUG grapevine::protocol::gossip: Broadcasting message to 3 peers
```

### Viewing Connected Peers

The CLI logs peer connection status every 10 seconds:

```bash
2025-11-04T10:00:10.000000Z  INFO grapevine: Connected to 5 peer(s)
```

For detailed peer information, use debug logging:

```bash
cargo run --bin grapevine -- --log-level debug
```

You'll see:

- Peer connection events
- Peer discovery messages
- Peer health status updates

## Leaving the Network

To gracefully leave the network, press `Ctrl+C`:

```bash
^C
2025-11-04T10:00:30.000000Z  INFO grapevine: Received shutdown signal, gracefully shutting down...
2025-11-04T10:00:30.500000Z  INFO grapevine: Node shutdown complete. Goodbye!
```

During shutdown, the node:

1. Sends `Goodbye` messages to all connected peers
2. Stops accepting new messages
3. Waits for in-flight messages to complete (max 500ms)
4. Closes all connections
5. Cleans up resources

## Advanced Usage

### Custom Configuration

Fine-tune the gossip protocol parameters:

```bash
cargo run --bin grapevine -- \
  --host 0.0.0.0 \
  --port 9000 \
  --gossip-interval 3 \
  --fanout 5 \
  --max-peers 100 \
  --log-level debug
```

**Parameters explained:**

- `--host` / `-H`: Network interface to bind to
  - `127.0.0.1`: Local only (default)
  - `0.0.0.0`: All interfaces (for remote connections)

- `--port` / `-p`: TCP port to listen on (default: 8000)

- `--peer` / `-b`: Bootstrap peer addresses (can specify multiple)

- `--gossip-interval` / `-g`: Heartbeat interval in seconds (default: 5)
  - Lower values = faster propagation, higher network traffic
  - Higher values = slower propagation, lower network traffic

- `--fanout` / `-f`: Number of peers to gossip to per round (default: 3)
  - Higher values = better reliability, more bandwidth
  - Lower values = less bandwidth, potential message loss

- `--max-peers` / `-m`: Maximum peer connections (default: 50)
  - Must be >= fanout

- `--log-level` / `-l`: Logging verbosity (trace, debug, info, warn, error)

### Multi-Node Local Cluster

Create a local 5-node cluster for testing:

**Terminal 1 - Seed node:**

```bash
cargo run --bin grapevine -- --port 8000
```

**Terminal 2 - Node 2:**

```bash
cargo run --bin grapevine -- --port 8001 --peer 127.0.0.1:8000
```

**Terminal 3 - Node 3:**

```bash
cargo run --bin grapevine -- --port 8002 --peer 127.0.0.1:8000
```

**Terminal 4 - Node 4:**

```bash
cargo run --bin grapevine -- --port 8003 --peer 127.0.0.1:8000,127.0.0.1:8001
```

**Terminal 5 - Node 5:**

```bash
cargo run --bin grapevine -- --port 8004 --peer 127.0.0.1:8000,127.0.0.1:8001
```

Watch as messages propagate through the network! Each node will receive heartbeats from every other node (though not every heartbeat due to probabilistic forwarding).

### Tuning Network Parameters

For different network scenarios:

**Small, reliable network (<=10 nodes):**

```bash
cargo run --bin grapevine -- \
  --fanout 5 \
  --gossip-interval 3 \
  --max-peers 10
```

- High fanout ensures reliability
- Shorter interval for faster propagation
- Low max peers for small network

**Large, bandwidth-constrained network (100+ nodes):**

```bash
cargo run --bin grapevine -- \
  --fanout 3 \
  --gossip-interval 10 \
  --max-peers 20
```

- Lower fanout to reduce bandwidth
- Longer interval to reduce traffic
- Moderate max peers for scalability

**High-throughput network:**

```bash
cargo run --bin grapevine -- \
  --fanout 7 \
  --gossip-interval 2 \
  --max-peers 100
```

- High fanout for message redundancy
- Short interval for low latency
- High max peers for better connectivity

## Limitations

### Current CLI Limitations

1. **No Direct Peer-to-Peer Messaging**: The CLI demonstrates broadcast-only gossip. To send messages to specific peers, you need to use the library API directly in your application.

2. **No Interactive Message Input**: The CLI broadcasts automated heartbeats. For custom message input, you can:
   - Modify the CLI source code
   - Use the library API in your own application
   - Build a custom CLI with interactive input

3. **No Persistent State**: Nodes don't persist peer information or messages across restarts.

### Protocol Limitations

1. **Best-Effort Delivery**: The gossip protocol uses probabilistic forwarding (70% default). Some messages may not reach all nodes through epidemic broadcast alone, but anti-entropy ensures eventual consistency.

2. **No Delivery Guarantees**: Messages are not acknowledged. Use the library API to implement application-level acknowledgments if needed.

3. **No Message Ordering**: Messages may arrive in different orders at different nodes.

## Troubleshooting

### Node won't start

Error: "Address already in use"

```bash
Error: Os { code: 48, kind: AddrInUse, message: "Address already in use" }
```

**Solution**: Another process is using the port. Choose a different port:

```bash
cargo run --bin grapevine -- --port 8001
```

### Can't connect to peers

**Symptom**: Node starts but doesn't connect to bootstrap peers

**Possible causes:**

1. Bootstrap peer is not running - verify with `netstat` or `lsof`
2. Firewall blocking connections - check firewall rules
3. Wrong IP address - ensure bootstrap peer address is correct
4. Network partition - check network connectivity

**Debug:**

```bash
# Use debug logging to see connection attempts
cargo run --bin grapevine -- --peer 127.0.0.1:8000 --log-level debug
```

Look for:

```bash
DEBUG grapevine::protocol::gossip: Failed to connect to peer 127.0.0.1:8000
```

### Not receiving messages

**Symptom**: Node connects but doesn't receive heartbeats from other nodes

**Possible causes:**

1. Probabilistic forwarding - with 70% forward probability, some messages are dropped
2. Network partitioned - nodes can't reach each other
3. TTL exhausted - message TTL reached 0 before reaching this node

**Solutions:**

1. Increase fanout for better propagation:

   ```bash
   cargo run --bin grapevine -- --fanout 5
   ```

2. Wait for anti-entropy sync (runs every 30 seconds by default)

3. Check debug logs:

   ```bash
   cargo run --bin grapevine -- --log-level debug
   ```

### High CPU or bandwidth usage

**Symptom**: Node consuming excessive resources

**Causes:**

- Too many peers
- Too short gossip interval
- Too high fanout

**Solutions:**

```bash
# Reduce resource usage
cargo run --bin grapevine -- \
  --gossip-interval 10 \
  --fanout 2 \
  --max-peers 20
```

## Next Steps

- **Use the Library**: For production applications, use the Grapevine library API directly. See [examples/](../examples/) for code samples.

- **Read the Protocol**: Understand how gossip works in [protocol.md](protocol.md).

- **Check Architecture**: Learn about internals in [architecture.md](architecture.md).

- **Explore Examples**:
  - `examples/simple_node.rs` - Basic single node setup
  - `examples/multi_node_cluster.rs` - Multi-node cluster
  - `examples/custom_config.rs` - Advanced configuration

## Support

- **Issues**: Report bugs at <https://github.com/kobby-pentangeli/grapevine/issues>
- **Discussions**: Ask questions in GitHub Discussions
- **Documentation**: Full API docs at <https://docs.rs/grapevine>
