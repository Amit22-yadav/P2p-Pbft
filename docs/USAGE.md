# P2P PBFT Blockchain - Usage Guide

This guide covers how to run the blockchain network, manage accounts, and perform transactions.

## Prerequisites

Build the project in release mode:

```bash
cargo build --release
```

This creates two binaries:
- `./target/release/p2p-pbft` - Main blockchain node
- `./target/release/wallet` - Account and transaction management

## Quick Start

### 1. Start the Network

```bash
./scripts/run_blockchain.sh start
```

This starts a 4-node PBFT network with:
- P2P ports: 9000-9003
- RPC ports: 8545-8548
- Each validator funded with 1,000,000 tokens

### 2. Start the Web Explorer (Optional)

```bash
./scripts/serve_frontend.sh
```

Then open http://localhost:3000 in your browser.

### 3. Check Status

```bash
./scripts/client.sh status
```

### 4. Run the Demo

```bash
./scripts/client.sh demo
```

The demo will:
1. Check node connectivity
2. Display validator accounts and balances
3. Create a new account
4. Transfer funds using a signed transaction
5. Wait for block confirmation
6. Show final balances

---

## Network Management

### Start Network

```bash
# Default 4-node network
./scripts/run_blockchain.sh start

# Custom number of nodes
NUM_NODES=7 ./scripts/run_blockchain.sh start

# Single node (development)
./scripts/run_blockchain.sh single
```

### Stop Network

```bash
./scripts/run_blockchain.sh stop
```

### Check Node Status

```bash
./scripts/run_blockchain.sh status
```

### View Logs

```bash
# Node 0 logs
./scripts/run_blockchain.sh logs 0

# Node 2 logs
./scripts/run_blockchain.sh logs 2
```

### Clean Data

```bash
./scripts/run_blockchain.sh clean
```

### Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `NUM_NODES` | 4 | Number of validator nodes |
| `BASE_P2P_PORT` | 9000 | Starting P2P port |
| `BASE_RPC_PORT` | 8545 | Starting RPC port |
| `DATA_DIR` | ./data | Data directory |
| `CHAIN_ID` | pbft-chain | Chain identifier |
| `INITIAL_BALANCE` | 1000000 | Initial balance per validator |

---

## Account Management

### View Validator Accounts

Validators are pre-funded accounts with deterministic keys:

```bash
./scripts/client.sh validators
```

Output:
```
Validator 0:
  Node ID: 3b6a27bcceb6a42d...
  Address: 0x7e5f4552091a69125d5dfcb7b8c2659029395bdf
  Balance: 1000000
  Nonce:   0

Validator 1:
  ...
```

### Create New Account

```bash
# Random account
./scripts/client.sh new-account

# From specific seed (deterministic)
./scripts/client.sh new-from-seed deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef
```

### Check Balance

```bash
./scripts/client.sh balance 0x7e5f4552091a69125d5dfcb7b8c2659029395bdf
```

### Check Nonce

```bash
./scripts/client.sh nonce 0x7e5f4552091a69125d5dfcb7b8c2659029395bdf
```

---

## Transactions

### Transfer Funds

```bash
./scripts/client.sh transfer <sender-seed> <recipient-address> <amount> [nonce]
```

**Example**: Transfer 1000 tokens from Validator 0 to another address:

```bash
# Validator 0's seed is 64 zeros
./scripts/client.sh transfer \
  0000000000000000000000000000000000000000000000000000000000000000 \
  0x2b5ad5c4795c026514f8317c7a215e218dccd6cf \
  1000
```

The script will:
1. Derive the sender's address from the seed
2. Get the current nonce (if not provided)
3. Create and sign the transaction
4. Submit via RPC

### Using the Wallet Directly

For more control, use the wallet binary:

```bash
# Show account info
./target/release/wallet account --seed 0000000000000000000000000000000000000000000000000000000000000000

# Create signed transaction (offline)
./target/release/wallet transfer \
  --seed 0000000000000000000000000000000000000000000000000000000000000000 \
  --to 0x2b5ad5c4795c026514f8317c7a215e218dccd6cf \
  --value 1000 \
  --nonce 0

# Create and submit transaction
./target/release/wallet send \
  --seed 0000000000000000000000000000000000000000000000000000000000000000 \
  --to 0x2b5ad5c4795c026514f8317c7a215e218dccd6cf \
  --value 1000 \
  --nonce 0 \
  --rpc http://localhost:8545
```

---

## Querying the Blockchain

### Get Current Height

```bash
./scripts/client.sh height
```

### Get Block by Number

```bash
./scripts/client.sh block 0    # Genesis block
./scripts/client.sh block 1    # Block 1
```

### Get Transaction by Hash

```bash
./scripts/client.sh tx 0xabcd...
```

### Get Mempool Status

```bash
./scripts/client.sh mempool
```

### Get Node Status

```bash
./scripts/client.sh status
```

---

## RPC API

The blockchain exposes a JSON-RPC API on each node's RPC port.

### Using curl

```bash
# Get block height
curl -s http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Get balance
curl -s http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x7e5f..."],"id":1}'

# Get node status
curl -s http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"pbft_getStatus","params":[],"id":1}'
```

### Using the RPC Script

```bash
./scripts/rpc.sh status
./scripts/rpc.sh height
./scripts/rpc.sh block 0
./scripts/rpc.sh balance 0x7e5f...
./scripts/rpc.sh nonce 0x7e5f...
./scripts/rpc.sh mempool
```

### Available RPC Methods

| Method | Parameters | Description |
|--------|------------|-------------|
| `eth_blockNumber` | - | Get current block height |
| `eth_getBalance` | `address` | Get account balance |
| `eth_getTransactionCount` | `address` | Get account nonce |
| `eth_getBlockByNumber` | `height` | Get block by height |
| `eth_getBlockByHash` | `hash` | Get block by hash |
| `eth_getTransactionByHash` | `hash` | Get transaction |
| `chain_submitTransaction` | `{from, to, value, nonce, signature}` | Submit signed transaction |
| `pbft_getStatus` | - | Get consensus status |
| `pbft_getMempool` | - | Get mempool contents |

---

## Validator Seeds

For testing, validators use deterministic seeds based on their index:

| Validator | Seed (hex) |
|-----------|------------|
| 0 | `0000000000000000000000000000000000000000000000000000000000000000` |
| 1 | `0100000000000000000000000000000000000000000000000000000000000000` |
| 2 | `0200000000000000000000000000000000000000000000000000000000000000` |
| 3 | `0300000000000000000000000000000000000000000000000000000000000000` |

Use `./scripts/client.sh validators` to see their addresses and balances.

---

## Web Explorer

A web-based blockchain explorer is included for easy interaction with the blockchain.

### Starting the Explorer

```bash
./scripts/serve_frontend.sh
```

This starts a web server on port 3000 (default). Open http://localhost:3000 in your browser.

### Features

**Dashboard**
- Real-time block height and mempool status
- Quick balance check
- Recent blocks view

**Wallet**
- Generate new accounts (random or from seed)
- View account details from seed
- Check balance and nonce

**Transfer**
- Send funds between accounts
- Preview transactions before sending
- Automatic nonce detection
- Transaction status lookup

**Explorer**
- Browse blocks by number
- View block details and transactions
- Monitor mempool

**Validators**
- View all pre-funded validator accounts
- Copy seeds/addresses
- Quick transfer setup

### Configuration

To use a different RPC endpoint, change the URL in the top-right corner of the interface.

---

## Troubleshooting

### Node Won't Start

1. Check if ports are in use:
   ```bash
   lsof -i :9000
   lsof -i :8545
   ```

2. Stop existing nodes:
   ```bash
   ./scripts/run_blockchain.sh stop
   ```

3. Clean data and restart:
   ```bash
   ./scripts/run_blockchain.sh clean
   ./scripts/run_blockchain.sh start
   ```

### Transaction Not Confirming

1. Check mempool:
   ```bash
   ./scripts/client.sh mempool
   ```

2. Check if nodes are connected:
   ```bash
   ./scripts/run_blockchain.sh status
   ```

3. Verify the primary node is producing blocks (check logs):
   ```bash
   ./scripts/run_blockchain.sh logs 0
   ```

### Invalid Nonce Error

Get the current nonce and use it:
```bash
./scripts/client.sh nonce <your-address>
./scripts/client.sh transfer <seed> <to> <value> <correct-nonce>
```

### Connection Refused

Ensure the network is running:
```bash
./scripts/run_blockchain.sh status
```

If all nodes show "Offline", restart the network:
```bash
./scripts/run_blockchain.sh start
```


<!-- for i in {1..10}; do ./client.sh demo; done -->