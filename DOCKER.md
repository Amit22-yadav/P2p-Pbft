# Docker Setup for P2P PBFT Blockchain

This guide explains how to run the P2P PBFT blockchain using Docker.

## Quick Start

### Start the entire network (4 nodes + explorer)

```bash
docker-compose up -d
```

This will:
- Build the Docker image
- Start 4 validator nodes
- Start the web explorer on port 3000

### Access Points

| Service    | Port  | Description                |
|------------|-------|----------------------------|
| Node 0 RPC | 8545  | Primary node RPC endpoint  |
| Node 1 RPC | 8546  | Validator 1 RPC endpoint   |
| Node 2 RPC | 8547  | Validator 2 RPC endpoint   |
| Node 3 RPC | 8548  | Validator 3 RPC endpoint   |
| Explorer   | 3000  | Web UI                     |
| Node 0 P2P | 9000  | P2P networking             |
| Node 1 P2P | 9001  | P2P networking             |
| Node 2 P2P | 9002  | P2P networking             |
| Node 3 P2P | 9003  | P2P networking             |

### View the Explorer

Open http://localhost:3000 in your browser.

### Check Node Status

```bash
# Check all nodes
curl -s http://localhost:8545 -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"pbft_getStatus","params":[],"id":1}' | jq

# Check specific node (e.g., node 1)
curl -s http://localhost:8546 -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"pbft_getStatus","params":[],"id":1}' | jq
```

### View Logs

```bash
# All services
docker-compose logs -f

# Specific node
docker-compose logs -f node0

# Explorer
docker-compose logs -f explorer
```

### Stop the Network

```bash
docker-compose down
```

### Stop and Remove Data

```bash
docker-compose down -v
```

## Building the Image

### Build only

```bash
docker build -t p2p-pbft .
```

### Rebuild after code changes

```bash
docker-compose build --no-cache
docker-compose up -d
```

## Running Individual Components

### Run a single node

```bash
docker run -d \
  --name pbft-node0 \
  -e NODE_INDEX=0 \
  -e NUM_NODES=4 \
  -p 9000:9000 \
  -p 8545:8545 \
  p2p-pbft
```

### Run the wallet CLI

```bash
# Generate a new account
docker run --rm p2p-pbft wallet new

# Check validators
docker run --rm p2p-pbft wallet validators

# Create account from seed
docker run --rm p2p-pbft wallet account --seed 0000000000000000000000000000000000000000000000000000000000000000
```

### Run keygen command

```bash
docker run --rm p2p-pbft keygen
docker run --rm p2p-pbft keygen --seed 0000000000000000000000000000000000000000000000000000000000000000
```

## Customization

### Environment Variables

| Variable         | Default      | Description                    |
|------------------|--------------|--------------------------------|
| NODE_INDEX       | 0            | Validator index (0-3)          |
| NUM_NODES        | 4            | Total number of validators     |
| P2P_PORT         | 9000         | P2P networking port            |
| RPC_PORT         | 8545         | JSON-RPC server port           |
| DATA_DIR         | /app/data    | Data storage directory         |
| CHAIN_ID         | pbft-chain   | Blockchain identifier          |
| INITIAL_BALANCE  | 1000000      | Initial validator balance      |
| BOOTSTRAP_NODES  | (empty)      | Space-separated list of peers  |
| RUST_LOG         | info         | Log level (debug, info, warn)  |

### Run with Custom Configuration

```bash
docker run -d \
  --name my-node \
  -e NODE_INDEX=0 \
  -e NUM_NODES=7 \
  -e INITIAL_BALANCE=5000000 \
  -e RUST_LOG=debug \
  -p 9000:9000 \
  -p 8545:8545 \
  -v my-data:/app/data \
  p2p-pbft
```

### Scale to More Nodes

Edit `docker-compose.yml` to add more nodes, or use:

```bash
# Create a docker-compose.override.yml for additional nodes
cat > docker-compose.override.yml << EOF
version: '3.8'
services:
  node4:
    build: .
    container_name: pbft-node4
    environment:
      - NODE_INDEX=4
      - NUM_NODES=5
      - BOOTSTRAP_NODES=node0:9000 node1:9000 node2:9000 node3:9000
    ports:
      - "9004:9000"
      - "8549:8545"
    networks:
      - pbft-network
EOF

docker-compose up -d
```

## Troubleshooting

### Nodes not connecting

1. Check if all nodes are running:
   ```bash
   docker-compose ps
   ```

2. Check node logs for errors:
   ```bash
   docker-compose logs node0 | tail -50
   ```

3. Verify network connectivity:
   ```bash
   docker network inspect p2p-pbft_pbft-network
   ```

### RPC not responding

1. Check if the container is healthy:
   ```bash
   docker inspect pbft-node0 --format='{{.State.Health.Status}}'
   ```

2. Check RPC is listening:
   ```bash
   docker exec pbft-node0 netstat -tlnp | grep 8545
   ```

### Data persistence

Data is stored in Docker volumes:
- `node0-data`, `node1-data`, `node2-data`, `node3-data`

To back up data:
```bash
docker run --rm -v p2p-pbft_node0-data:/data -v $(pwd):/backup alpine tar czf /backup/node0-backup.tar.gz /data
```

### Complete reset

```bash
docker-compose down -v
docker rmi p2p-pbft_node0 p2p-pbft_explorer
docker-compose up -d --build
```
