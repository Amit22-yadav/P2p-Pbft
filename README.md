# P2P Network with PBFT Consensus

A production-ready Rust implementation of a peer-to-peer network using the **Practical Byzantine Fault Tolerance (PBFT)** consensus algorithm. This project demonstrates distributed systems concepts including networking, cryptography, and consensus mechanisms.

## Table of Contents

- [Features](#features)
- [Architecture Overview](#architecture-overview)
- [Requirements](#requirements)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Project Structure](#project-structure)
- [How It Works](#how-it-works)
- [Configuration](#configuration)
- [API Reference](#api-reference)
- [Testing](#testing)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)
- [License](#license)

## Features

- **Full PBFT Implementation**: Complete 3-phase consensus (Pre-prepare, Prepare, Commit)
- **Byzantine Fault Tolerance**: Tolerates up to f faulty nodes in a 3f+1 network
- **P2P Networking**: TCP-based peer discovery and communication
- **Cryptographic Security**: Ed25519 digital signatures for message authentication
- **View Change Protocol**: Automatic leader election on primary failure
- **Checkpointing**: Garbage collection and state synchronization
- **Async Runtime**: Built on Tokio for high-performance I/O
- **CLI Interface**: Interactive commands for testing and monitoring

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         Application Layer                        │
│                    (Request Submission & Execution)              │
├─────────────────────────────────────────────────────────────────┤
│                       PBFT Consensus Engine                      │
│  ┌───────────┐   ┌───────────┐   ┌───────────┐   ┌───────────┐ │
│  │Pre-Prepare│ → │  Prepare  │ → │  Commit   │ → │  Execute  │ │
│  │  (Primary)│   │(All Nodes)│   │(All Nodes)│   │(All Nodes)│ │
│  └───────────┘   └───────────┘   └───────────┘   └───────────┘ │
├─────────────────────────────────────────────────────────────────┤
│                      P2P Network Layer                           │
│  ┌──────────────┐  ┌─────────────────┐  ┌────────────────────┐ │
│  │TCP Connections│  │ Peer Discovery  │  │ Message Broadcast  │ │
│  └──────────────┘  └─────────────────┘  └────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│                    Cryptography Layer                            │
│              (Ed25519 Signing & Verification)                    │
└─────────────────────────────────────────────────────────────────┘
```

## Requirements

- **Rust**: 1.70+ (install from [rustup.rs](https://rustup.rs/))
- **Operating System**: macOS, Linux, or Windows
- **Network**: Local network or internet connectivity for distributed setup

## Installation

### From Source

```bash
# Clone the repository
git clone <repository-url>
cd p2p-pbft

# Build in release mode
cargo build --release

# The binary will be at ./target/release/p2p-pbft
```

### Using the Run Script

```bash
# Make the script executable
chmod +x run.sh

# Build the project
./run.sh build
```

## Quick Start

### Start a 4-Node Network (Recommended for Testing)

**Option 1: Using the run script (easiest)**

```bash
# Start all 4 nodes in separate terminal windows
./run.sh network

# Or start in background
./run.sh network-bg
./run.sh status
```

**Option 2: Manual startup in 4 terminals**

```bash
# Terminal 1 - Primary Node (Node 0)
cargo run --release -- start -p 8000 -i 0 -n 4

# Terminal 2 - Node 1
cargo run --release -- start -p 8001 -i 1 -n 4 -b 127.0.0.1:8000

# Terminal 3 - Node 2
cargo run --release -- start -p 8002 -i 2 -n 4 -b 127.0.0.1:8000

# Terminal 4 - Node 3
cargo run --release -- start -p 8003 -i 3 -n 4 -b 127.0.0.1:8000
```

### Submit a Request

In the primary node's terminal:
```
> submit hello world
```

Watch the consensus process across all nodes!

## Usage

### Command Line Interface

```bash
p2p-pbft <COMMAND>

Commands:
  start    Start a PBFT node
  keygen   Generate a new keypair
  help     Print help information
```

### Start Command Options

```bash
p2p-pbft start [OPTIONS]

Options:
  -p, --port <PORT>        Port to listen on [default: 8000]
  -s, --seed <SEED>        Seed for deterministic key generation (hex, 32 bytes)
  -b, --bootstrap <ADDR>   Bootstrap peer addresses (can be repeated)
  -n, --nodes <COUNT>      Total number of nodes in network [default: 4]
  -i, --index <INDEX>      Node index for deterministic setup (0-based)
```

### Interactive Commands (While Running)

| Command | Shortcut | Description |
|---------|----------|-------------|
| `submit <data>` | `s` | Submit a request to the network |
| `status` | `st` | Show node status (view, sequence, primary) |
| `peers` | `p` | List all replicas in the network |
| `help` | `h` | Show available commands |
| `quit` | `q` | Exit the node |

### Run Script Commands

```bash
./run.sh build        # Build the project
./run.sh node <0-3>   # Start a single node
./run.sh network      # Start 4 nodes in terminal windows
./run.sh network-bg   # Start 4 nodes in background
./run.sh status       # Show network status
./run.sh stop         # Stop all background nodes
./run.sh keygen       # Generate a keypair
./run.sh clean        # Clean build artifacts
./run.sh help         # Show help
```

## Project Structure

```
p2p-pbft/
├── Cargo.toml           # Project dependencies and metadata
├── README.md            # This file
├── DOCUMENTATION.md     # Detailed technical documentation
├── run.sh               # Helper script for running the network
├── logs/                # Log files (created at runtime)
└── src/
    ├── main.rs          # CLI entry point and node orchestration
    ├── lib.rs           # Library exports
    ├── crypto.rs        # Ed25519 cryptographic operations
    ├── message.rs       # Network and consensus message types
    ├── network.rs       # P2P TCP networking layer
    ├── node.rs          # High-level node abstraction
    └── pbft.rs          # PBFT consensus state machine
```

## How It Works

### PBFT Consensus Flow

1. **Client Request**: A client sends a request to the primary node
2. **Pre-Prepare**: Primary assigns sequence number and broadcasts to all replicas
3. **Prepare**: Each replica validates and broadcasts prepare message
4. **Commit**: After receiving 2f+1 prepares, replicas broadcast commit
5. **Execute**: After receiving 2f+1 commits, request is executed

### Fault Tolerance

| Network Size (n) | Max Faulty Nodes (f) | Quorum Required |
|------------------|----------------------|-----------------|
| 4                | 1                    | 3               |
| 7                | 2                    | 5               |
| 10               | 3                    | 7               |
| 3f + 1           | f                    | 2f + 1          |

### View Change

When the primary fails or becomes Byzantine:
1. Replicas detect timeout (no activity from primary)
2. Replicas broadcast ViewChange messages
3. After 2f+1 ViewChanges, new primary sends NewView
4. Network resumes with new primary

## Configuration

### Environment Variables

Currently, all configuration is done via command-line arguments.

### PBFT Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| View change timeout | 10s | Time before triggering view change |
| Checkpoint interval | 100 | Requests between checkpoints |
| Message buffer | 1000 | Channel buffer size |

## API Reference

### Library Usage

```rust
use p2p_pbft::{NodeBuilder, Request, PbftConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a node
    let mut node = NodeBuilder::new(8000)
        .with_bootstrap_peer("127.0.0.1:8001".to_string())
        .build();

    // Start the node (runs event loop)
    node.start().await?;

    Ok(())
}
```

### Key Types

- `Node`: High-level node combining network and consensus
- `Network`: P2P networking layer
- `PbftConsensus`: Consensus engine
- `PbftState`: Replica state
- `Request`: Client request/transaction
- `KeyPair`: Ed25519 signing keypair

## Testing

### Run Unit Tests

```bash
cargo test
```

### Integration Testing

1. Start a 4-node network
2. Submit requests from any node
3. Verify all nodes execute in same order
4. Kill the primary and observe view change

### Stress Testing

```bash
# In primary node terminal, submit multiple requests
> submit tx1
> submit tx2
> submit tx3
```

## Troubleshooting

### Common Issues

**"Connection refused" error**
- Ensure the bootstrap node is running before starting other nodes
- Check that ports are not in use: `lsof -i :8000`

**"Rust/Cargo is not installed"**
- Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

**Nodes not reaching consensus**
- Ensure at least 3 nodes are running (for n=4 network)
- Check network connectivity between nodes

**View change not happening**
- Default timeout is 10 seconds
- Verify nodes can communicate with each other

### Debug Mode

For verbose logging:
```bash
RUST_LOG=debug cargo run -- start -p 8000 -i 0 -n 4
```

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Write tests for new functionality
4. Submit a pull request

## License

MIT License - see LICENSE file for details.

## Further Reading

- [DOCUMENTATION.md](DOCUMENTATION.md) - Detailed technical documentation
- [PBFT Paper](https://pmg.csail.mit.edu/papers/osdi99.pdf) - Original PBFT paper by Castro and Liskov
- [Rust Async Book](https://rust-lang.github.io/async-book/) - Async programming in Rust
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial) - Tokio async runtime

## Acknowledgments

- Original PBFT algorithm by Miguel Castro and Barbara Liskov
- Rust community for excellent async ecosystem
- Ed25519 implementation by dalek-cryptography team
