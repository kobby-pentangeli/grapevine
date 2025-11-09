# Grapevine CLI Usage Guide

This guide provides comprehensive instructions for using the Grapevine interactive CLI to create and manage gossip networks, broadcast messages, and send direct messages to specific peers.

## Installation

Build the Grapevine CLI from source:

```bash
# Clone the repository
git clone https://github.com/kobby-pentangeli/grapevine
cd grapevine
```

## Quick Start

Start your first node and begin sending messages immediately:

```bash
# Terminal 1: Start seed node
cargo run
```

You'll see the interactive interface:

```bash
┌──────────────────────────────────────────────────────────┐
│            Grapevine Interactive Gossip Client           │
└──────────────────────────────────────────────────────────┘

✓ Node started on 127.0.0.1:8000
  Gossip interval: 5s
  Fanout: 3
  Max peers: 50

Type /help for available commands or /quit to exit

grapevine@127.0.0.1:8000>
```

In another terminal, join the network:

```bash
# Terminal 2: Join the network
cargo run -- --port 8001 --peer 127.0.0.1:8000
```

Now you can exchange messages interactively!

```bash
grapevine@127.0.0.1:8000> /broadcast Hello network!
✓ Message broadcasted

grapevine@127.0.0.1:8000> /send 127.0.0.1:8001 Private message
✓ Message sent to 127.0.0.1:8001
```

## Interactive Interface

The CLI provides an interactive shell with colored output and real-time message display. When messages arrive, they're displayed immediately without interrupting your input.

### Available Commands

| Command                  | Description                               | Example                         |
| ------------------------ | ----------------------------------------- | ------------------------------- |
| `/broadcast <message>`   | Broadcast message to all peers via gossip | `/broadcast Hello everyone!`    |
| `/send <peer> <message>` | Send direct message to specific peer      | `/send 127.0.0.1:8001 Hi there` |
| `/peers`                 | List all connected peers                  | `/peers`                        |
| `/status`                | Show detailed node status                 | `/status`                       |
| `/help`                  | Display help message                      | `/help`                         |
| `/quit` or `/exit`       | Gracefully exit the node                  | `/quit`                         |

### Command Shortcuts

For faster typing, use these shortcuts:

- `/b` -> `/broadcast`
- `/s` -> `/send`
- `/p` -> `/peers`
- `/st` -> `/status`
- `/h` -> `/help`
- `/q` -> `/quit`

## Starting a Node

### Starting the First Node (Seed Node)

The first node in a network acts as the **seed node** that other nodes connect to:

```bash
# Start on default port (8000)
cargo run

# Start on custom port
cargo run -- --port 9000

# Bind to all interfaces (for remote connections)
cargo run -- --host 0.0.0.0 --port 9000
```

The seed node will display:

```bash
┌──────────────────────────────────────────────────────────┐
│            Grapevine Interactive Gossip Client           │
└──────────────────────────────────────────────────────────┘

✓ Node started on 127.0.0.1:8000
  Gossip interval: 5s
  Fanout: 3
  Max peers: 50

Type /help for available commands or /quit to exit

grapevine@127.0.0.1:8000>
```

### Joining an Existing Network

To join an existing network, specify one or more **bootstrap peers**:

```bash
# Join with single bootstrap peer
cargo run -- --port 8001 --peer 127.0.0.1:8000

# Join with multiple bootstrap peers (comma-separated)
cargo run -- --port 8002 \
  --peer 127.0.0.1:8000,127.0.0.1:8001
```

The node will:

1. Connect to the specified bootstrap peer(s)
2. Discover other peers in the network
3. Begin participating in message gossip
4. Display connection status

### Starting with Environment Variables

Use environment variables for easier deployment:

```bash
# Set via environment
export BIND_HOST=0.0.0.0
export BIND_PORT=9000
export BOOTSTRAP_PEERS=127.0.0.1:8000,127.0.0.1:8001
export GOSSIP_INTERVAL_SECS=5
export FANOUT=3
export MAX_PEERS=50
export RUST_LOG=info

cargo run
```

Or copy `.env.example` to `.env` and customize:

```bash
# Copy
cp .env.example .env

# Edit .env with your configuration
BIND_HOST=0.0.0.0
BIND_PORT=9000
BOOTSTRAP_PEERS=127.0.0.1:8000
GOSSIP_INTERVAL_SECS=5
FANOUT=3
MAX_PEERS=50
RUST_LOG=info

# Start node (automatically reads .env)
cargo run
```

## Sending Messages

### Broadcasting to All Peers

Use `/broadcast` to send messages to all nodes in the network via epidemic gossip:

```bash
grapevine@127.0.0.1:8000> /broadcast Hello everyone!
✓ Message broadcasted
```

**How it works:**

1. Message is sent to a random subset of connected peers (fanout)
2. Each receiving peer probabilistically forwards to their peers (70% default)
3. Message propagates through the network in O(log N) hops
4. Anti-entropy ensures eventual delivery even if epidemic broadcast misses some nodes

**Received messages are displayed automatically:**

```bash
grapevine@127.0.0.1:8001>
Message received from 127.0.0.1:8000: Hello everyone!
```

### Sending Direct Messages

Use `/send` to send messages directly to a specific peer without gossip propagation:

```bash
grapevine@127.0.0.1:8000> /send 127.0.0.1:8001 Private message for you
✓ Message sent to 127.0.0.1:8001
```

**How it works:**

1. Message is sent directly to the specified peer only
2. Message is NOT propagated to other peers
3. Recipient must be currently connected
4. Delivery is immediate (no gossip delay)

**Direct messages are received the same way:**

```bash
grapevine@127.0.0.1:8001>
Message received from 127.0.0.1:8000: Private message for you
```

## Network Management

### Viewing Connected Peers

Use `/peers` to see all currently connected peers:

```bash
grapevine@127.0.0.1:8000> /peers
Connected peers (3):
  • 127.0.0.1:8001
  • 127.0.0.1:8002
  • 127.0.0.1:8003
```

If no peers are connected:

```bash
grapevine@127.0.0.1:8000> /peers
No connected peers
```

### Checking Node Status

Use `/status` to view detailed node information:

```bash
grapevine@127.0.0.1:8000> /status

Node Status:
  Local address: 127.0.0.1:8000
  Connected peers: 3
  Gossip interval: 5s
  Fanout: 3
```

## Leaving the Network

To gracefully leave the network, use `/quit` or `/exit`:

```bash
grapevine@127.0.0.1:8000> /quit

2025-11-04T10:00:30.000000Z  INFO grapevine: Shutting down gracefully...
✓ Node shutdown complete. Goodbye!
```

Or press `Ctrl+C` (also triggers graceful shutdown).

**During shutdown:**

1. Sends `Goodbye` messages to all connected peers
2. Stops accepting new messages
3. Waits for in-flight messages (max 500ms)
4. Closes all peer connections
5. Cleans up resources

## Next Steps

- **Use the Library**: For custom applications, use the Grapevine library API directly. See [examples/](../examples/) for code samples.

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
