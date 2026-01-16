# P2P PBFT Blockchain

A full-featured blockchain implementation with PBFT (Practical Byzantine Fault Tolerance) consensus, written in Rust.

## Features

- **PBFT Consensus** - Byzantine fault-tolerant consensus supporting up to f faulty nodes (where n = 3f + 1)
- **Account-based State Model** - Ethereum-style accounts with balances and nonces
- **P2P Networking** - Decentralized peer-to-peer communication
- **JSON-RPC API** - Ethereum-compatible RPC interface
- **Web Explorer** - Browser-based blockchain explorer
- **Wallet CLI** - Command-line wallet for account management and transactions
- **Docker Support** - Containerized deployment with docker-compose
- **Persistent Storage** - RocksDB for blockchain data persistence

## Quick Start

### Prerequisites

- Rust 1.70+ (for building from source)
- OR Docker & Docker Compose

### Option 1: Run with Scripts

```bash
# Build
cargo build --release

# Start 4-node network + web explorer
./scripts/start.sh

# Access:
# - Explorer: http://localhost:3000
# - RPC: http://localhost:8545
```

### Option 2: Run with Docker

```bash
# Start entire network
docker-compose up -d

# Access:
# - Explorer: http://localhost:3000
# - RPC: http://localhost:8545

# Stop
docker-compose down
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Blockchain Node                         │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   JSON-RPC  │  │  P2P Network │  │   PBFT Consensus   │  │
│  │   Server    │  │   (TCP)      │  │   Engine           │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         │                │                     │             │
│         └────────────────┼─────────────────────┘             │
│                          │                                   │
│  ┌───────────────────────┴───────────────────────────────┐  │
│  │                    Chain Manager                       │  │
│  │  ┌─────────┐  ┌──────────┐  ┌─────────┐  ┌─────────┐  │  │
│  │  │ Mempool │  │ Executor │  │ Storage │  │ Accounts│  │  │
│  │  └─────────┘  └──────────┘  └─────────┘  └─────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
│                          │                                   │
│                    ┌─────┴─────┐                            │
│                    │  RocksDB  │                            │
│                    └───────────┘                            │
└─────────────────────────────────────────────────────────────┘
```

## Web Explorer

The built-in web explorer provides:

- **Dashboard** - Network status, block height, peer count
- **Blocks** - Browse and inspect blocks
- **Validators** - View validator accounts and balances
- **Transfer** - Generate transaction commands
- **Accounts** - Generate new accounts, lookup balances
- **Lookup** - Search transactions by hash

Access at http://localhost:3000 after starting the network.

## CLI Commands

### Blockchain Node

```bash
# Start a blockchain node
./target/release/p2p-pbft blockchain \
  --port 9000 \
  --rpc-port 8545 \
  --index 0 \
  --nodes 4

# Generate a new account
./target/release/p2p-pbft keygen

# Generate from seed
./target/release/p2p-pbft keygen --seed 0000...0000
```

### Wallet

```bash
# Create new account
./target/release/wallet new

# Show account from seed
./target/release/wallet account --seed <64-hex-chars>

# Send transaction
./target/release/wallet send \
  --seed <sender-seed> \
  --to <recipient-address> \
  --value 1000 \
  --nonce 0

# List validator accounts
./target/release/wallet validators
```

### Client Script

```bash
# Check node status
./scripts/client.sh status

# Get balance
./scripts/client.sh balance 0x...

# Transfer funds
./scripts/client.sh transfer <seed> <to-address> <amount>

# Run demo
./scripts/client.sh demo

# List validators
./scripts/client.sh validators
```

## JSON-RPC API

### Endpoints

| Method | Description |
|--------|-------------|
| `eth_blockNumber` | Get current block height |
| `eth_getBalance` | Get account balance |
| `eth_getTransactionCount` | Get account nonce |
| `eth_getBlockByNumber` | Get block by height |
| `eth_getBlockByHash` | Get block by hash |
| `eth_getTransactionByHash` | Get transaction details |
| `chain_submitTransaction` | Submit signed transaction |
| `pbft_getStatus` | Get consensus status |
| `pbft_getMempool` | Get mempool contents |

### Examples

```bash
# Get node status
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"pbft_getStatus","params":[],"id":1}'

# Get balance
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x139e3940e64b5491722088d9a0d741628fc826e0"],"id":1}'

# Get block
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0"],id":1}'
```

## Validators

Default network uses 4 deterministic validators:

| Index | Address | Initial Balance |
|-------|---------|-----------------|
| 0 | `0x139e3940e64b5491722088d9a0d741628fc826e0` | 1,000,000 |
| 1 | `0xfa4d86c3b551aa6cd7c3759d040c037ef2c6379f` | 1,000,000 |
| 2 | `0xe3c1b362c0df36f6b370b8b1479b67dad96392b2` | 1,000,000 |
| 3 | `0xdb0743e2dcba9ebf2419bde0881beea966689a26` | 1,000,000 |

Seeds are deterministic: validator N uses seed with index byte at position 0 (e.g., `0000...`, `0100...`, `0200...`).

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NUM_NODES` | 4 | Number of validator nodes |
| `BASE_P2P_PORT` | 9000 | Starting P2P port |
| `BASE_RPC_PORT` | 8545 | Starting RPC port |
| `FRONTEND_PORT` | 3000 | Web explorer port |
| `CHAIN_ID` | pbft-chain | Chain identifier |
| `INITIAL_BALANCE` | 1000000 | Initial validator balance |
| `RUST_LOG` | info | Log level (debug, info, warn, error) |

## Project Structure

```
p2p-pbft/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── lib.rs           # Library exports
│   ├── pbft.rs          # PBFT consensus
│   ├── chain.rs         # Chain management
│   ├── blockchain.rs    # Blockchain orchestrator
│   ├── execution.rs     # Transaction execution
│   ├── mempool.rs       # Transaction pool
│   ├── storage.rs       # RocksDB storage
│   ├── account.rs       # Account state
│   ├── rpc.rs           # JSON-RPC server
│   ├── network.rs       # P2P networking
│   ├── types.rs         # Core types
│   ├── crypto.rs        # Cryptographic operations
│   ├── merkle.rs        # Merkle tree
│   └── bin/
│       └── wallet.rs    # Wallet CLI
├── frontend/
│   └── index.html       # Web explorer
├── scripts/
│   ├── start.sh         # All-in-one starter
│   ├── run_blockchain.sh # Node runner
│   └── client.sh        # Client utilities
├── Dockerfile
├── docker-compose.yml
├── DOCKER.md            # Docker documentation
└── Cargo.toml
```

## PBFT Consensus

The implementation follows the classic PBFT protocol:

1. **Pre-prepare** - Primary proposes a block
2. **Prepare** - Validators broadcast prepare messages
3. **Commit** - After 2f+1 prepares, validators broadcast commit
4. **Execute** - After 2f+1 commits, block is executed and committed

### Fault Tolerance

| Network Size (n) | Max Faulty (f) | Quorum |
|------------------|----------------|--------|
| 4 | 1 | 3 |
| 7 | 2 | 5 |
| 10 | 3 | 7 |

Formula: n = 3f + 1, Quorum = 2f + 1

## Docker

See [DOCKER.md](DOCKER.md) for detailed Docker instructions.

```bash
# Start network
docker-compose up -d

# View logs
docker-compose logs -f

# Stop
docker-compose down

# Reset (remove data)
docker-compose down -v
```

## Development

### Build

```bash
cargo build --release
```

### Run Tests

```bash
cargo test
```

### Run Single Node (Development)

```bash
./scripts/run_blockchain.sh single
```

## License

MIT

## Author

Amit Kumar
