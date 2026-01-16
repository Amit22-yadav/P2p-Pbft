#!/bin/bash

# P2P PBFT Blockchain Runner Script
# This script starts a multi-node blockchain network

set -e

# Configuration
NUM_NODES=${NUM_NODES:-4}
BASE_P2P_PORT=${BASE_P2P_PORT:-9000}
BASE_RPC_PORT=${BASE_RPC_PORT:-8545}
DATA_DIR=${DATA_DIR:-"./data"}
CHAIN_ID=${CHAIN_ID:-"pbft-chain"}
INITIAL_BALANCE=${INITIAL_BALANCE:-1000000}

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Build the project first
echo -e "${BLUE}Building the project...${NC}"
cargo build --release

BINARY="./target/release/p2p-pbft"

if [ ! -f "$BINARY" ]; then
    echo -e "${RED}Binary not found at $BINARY${NC}"
    exit 1
fi

# Function to cleanup on exit
cleanup() {
    echo -e "\n${YELLOW}Shutting down all nodes...${NC}"
    pkill -f "p2p-pbft blockchain" 2>/dev/null || true
    exit 0
}

trap cleanup SIGINT SIGTERM

# Function to start a node
start_node() {
    local index=$1
    local p2p_port=$((BASE_P2P_PORT + index))
    local rpc_port=$((BASE_RPC_PORT + index))
    local node_data_dir="${DATA_DIR}/node${index}"
    local log_file="./logs/node${index}.log"

    # Build bootstrap list (connect to all previous nodes)
    local bootstrap_args=""
    for ((i=0; i<index; i++)); do
        bootstrap_args="$bootstrap_args --bootstrap 127.0.0.1:$((BASE_P2P_PORT + i))"
    done

    echo -e "${GREEN}Starting node $index...${NC}"
    echo "  P2P Port: $p2p_port"
    echo "  RPC Port: $rpc_port"
    echo "  Data Dir: $node_data_dir"
    echo "  Log File: $log_file"

    mkdir -p "$(dirname "$log_file")"
    mkdir -p "$node_data_dir"

    $BINARY blockchain \
        --port $p2p_port \
        --rpc-port $rpc_port \
        --data-dir "$node_data_dir" \
        --chain-id "$CHAIN_ID" \
        --nodes $NUM_NODES \
        --index $index \
        --initial-balance $INITIAL_BALANCE \
        $bootstrap_args \
        > "$log_file" 2>&1 &

    echo "  PID: $!"
    echo ""
}

# Parse command line arguments
case "${1:-start}" in
    start)
        echo -e "${BLUE}======================================${NC}"
        echo -e "${BLUE}  P2P PBFT Blockchain Network${NC}"
        echo -e "${BLUE}======================================${NC}"
        echo ""
        echo "Configuration:"
        echo "  Nodes:           $NUM_NODES"
        echo "  Chain ID:        $CHAIN_ID"
        echo "  P2P Ports:       $BASE_P2P_PORT - $((BASE_P2P_PORT + NUM_NODES - 1))"
        echo "  RPC Ports:       $BASE_RPC_PORT - $((BASE_RPC_PORT + NUM_NODES - 1))"
        echo "  Initial Balance: $INITIAL_BALANCE"
        echo ""

        # Create logs directory
        mkdir -p ./logs

        # Start all nodes
        for ((i=0; i<NUM_NODES; i++)); do
            start_node $i
            # Small delay between node starts
            sleep 1
        done

        echo -e "${GREEN}======================================${NC}"
        echo -e "${GREEN}  All nodes started!${NC}"
        echo -e "${GREEN}======================================${NC}"
        echo ""
        echo "RPC Endpoints:"
        for ((i=0; i<NUM_NODES; i++)); do
            echo "  Node $i: http://localhost:$((BASE_RPC_PORT + i))"
        done
        echo ""
        echo "Useful commands:"
        echo "  # Get chain status"
        echo "  curl -s http://localhost:$BASE_RPC_PORT -d '{\"jsonrpc\":\"2.0\",\"method\":\"pbft_getStatus\",\"params\":[],\"id\":1}' | jq"
        echo ""
        echo "  # Get block by number"
        echo "  curl -s http://localhost:$BASE_RPC_PORT -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"0\"],\"id\":1}' | jq"
        echo ""
        echo "  # View logs"
        echo "  tail -f ./logs/node0.log"
        echo ""
        echo -e "${YELLOW}Press Ctrl+C to stop all nodes${NC}"

        # Wait for all background processes
        wait
        ;;

    stop)
        echo -e "${YELLOW}Stopping all blockchain nodes...${NC}"
        pkill -f "p2p-pbft blockchain" 2>/dev/null || true
        echo -e "${GREEN}All nodes stopped.${NC}"
        ;;

    status)
        echo -e "${BLUE}Checking node status...${NC}"
        echo ""
        for ((i=0; i<NUM_NODES; i++)); do
            rpc_port=$((BASE_RPC_PORT + i))
            echo -n "Node $i (port $rpc_port): "
            response=$(curl -s --max-time 2 "http://localhost:$rpc_port" \
                -H "Content-Type: application/json" \
                -d '{"jsonrpc":"2.0","method":"pbft_getStatus","params":[],"id":1}' 2>/dev/null)
            if [ $? -eq 0 ] && [ -n "$response" ]; then
                height=$(echo "$response" | jq -r '.result.chain_height // "?"')
                is_primary=$(echo "$response" | jq -r '.result.is_primary // "?"')
                echo -e "${GREEN}Online${NC} - Height: $height, Primary: $is_primary"
            else
                echo -e "${RED}Offline${NC}"
            fi
        done
        ;;

    logs)
        node_index=${2:-0}
        log_file="./logs/node${node_index}.log"
        if [ -f "$log_file" ]; then
            tail -f "$log_file"
        else
            echo -e "${RED}Log file not found: $log_file${NC}"
        fi
        ;;

    clean)
        echo -e "${YELLOW}Cleaning up data and logs...${NC}"
        rm -rf "$DATA_DIR"
        rm -rf ./logs
        echo -e "${GREEN}Cleanup complete.${NC}"
        ;;

    init)
        echo -e "${BLUE}Initializing blockchain...${NC}"
        $BINARY init \
            --data-dir "$DATA_DIR" \
            --chain-id "$CHAIN_ID" \
            --nodes $NUM_NODES \
            --initial-balance $INITIAL_BALANCE
        ;;

    single)
        echo -e "${BLUE}Starting single node for development...${NC}"
        node_data_dir="${DATA_DIR}/dev"
        mkdir -p "$node_data_dir"

        $BINARY blockchain \
            --port $BASE_P2P_PORT \
            --rpc-port $BASE_RPC_PORT \
            --data-dir "$node_data_dir" \
            --chain-id "$CHAIN_ID" \
            --nodes 1 \
            --index 0 \
            --initial-balance $INITIAL_BALANCE
        ;;

    help|--help|-h)
        echo "P2P PBFT Blockchain Runner"
        echo ""
        echo "Usage: $0 [command]"
        echo ""
        echo "Commands:"
        echo "  start   - Start all nodes (default)"
        echo "  stop    - Stop all running nodes"
        echo "  status  - Check status of all nodes"
        echo "  logs N  - Tail logs for node N (default: 0)"
        echo "  clean   - Remove all data and logs"
        echo "  init    - Initialize genesis block"
        echo "  single  - Start a single node for development"
        echo "  help    - Show this help message"
        echo ""
        echo "Environment variables:"
        echo "  NUM_NODES       - Number of nodes (default: 4)"
        echo "  BASE_P2P_PORT   - Starting P2P port (default: 9000)"
        echo "  BASE_RPC_PORT   - Starting RPC port (default: 8545)"
        echo "  DATA_DIR        - Data directory (default: ./data)"
        echo "  CHAIN_ID        - Chain identifier (default: pbft-chain)"
        echo "  INITIAL_BALANCE - Initial balance per validator (default: 1000000)"
        echo ""
        echo "Examples:"
        echo "  $0 start                    # Start 4-node network"
        echo "  NUM_NODES=7 $0 start        # Start 7-node network"
        echo "  $0 single                   # Start single dev node"
        echo "  $0 status                   # Check all nodes"
        echo "  $0 logs 1                   # View node 1 logs"
        ;;

    *)
        echo -e "${RED}Unknown command: $1${NC}"
        echo "Run '$0 help' for usage information."
        exit 1
        ;;
esac
