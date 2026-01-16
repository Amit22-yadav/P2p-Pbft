#!/bin/bash

# RPC Helper Script for P2P PBFT Blockchain
# Usage: ./rpc.sh [method] [params...]

RPC_URL=${RPC_URL:-"http://localhost:8545"}

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

rpc_call() {
    local method=$1
    local params=$2

    curl -s "$RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":[$params],\"id\":1}" | jq .
}

case "${1:-help}" in
    status|st)
        echo -e "${BLUE}Getting node status...${NC}"
        rpc_call "pbft_getStatus" ""
        ;;

    height|h)
        echo -e "${BLUE}Getting block height...${NC}"
        rpc_call "eth_blockNumber" ""
        ;;

    block|b)
        height=${2:-0}
        echo -e "${BLUE}Getting block $height...${NC}"
        rpc_call "eth_getBlockByNumber" "\"$height\""
        ;;

    balance|bal)
        address=${2:-""}
        if [ -z "$address" ]; then
            echo "Usage: $0 balance <address>"
            exit 1
        fi
        echo -e "${BLUE}Getting balance for $address...${NC}"
        rpc_call "eth_getBalance" "\"$address\""
        ;;

    nonce|n)
        address=${2:-""}
        if [ -z "$address" ]; then
            echo "Usage: $0 nonce <address>"
            exit 1
        fi
        echo -e "${BLUE}Getting nonce for $address...${NC}"
        rpc_call "eth_getTransactionCount" "\"$address\""
        ;;

    tx)
        hash=${2:-""}
        if [ -z "$hash" ]; then
            echo "Usage: $0 tx <hash>"
            exit 1
        fi
        echo -e "${BLUE}Getting transaction $hash...${NC}"
        rpc_call "eth_getTransactionByHash" "\"$hash\""
        ;;

    mempool|mp)
        echo -e "${BLUE}Getting mempool status...${NC}"
        rpc_call "pbft_getMempool" ""
        ;;

    send)
        from=${2:-""}
        to=${3:-""}
        value=${4:-0}
        nonce=${5:-0}
        if [ -z "$from" ] || [ -z "$to" ]; then
            echo "Usage: $0 send <from> <to> <value> [nonce]"
            echo ""
            echo "Note: This creates an unsigned transaction request."
            echo "In production, transactions should be signed client-side."
            exit 1
        fi
        echo -e "${BLUE}Submitting transaction...${NC}"
        rpc_call "chain_submitTransaction" "{\"from\":\"$from\",\"to\":\"$to\",\"value\":$value,\"nonce\":$nonce}"
        ;;

    help|--help|-h|*)
        echo "P2P PBFT Blockchain RPC Client"
        echo ""
        echo "Usage: $0 <command> [args...]"
        echo ""
        echo "Commands:"
        echo "  status          - Get node status (pbft_getStatus)"
        echo "  height          - Get current block height"
        echo "  block [N]       - Get block by number (default: 0)"
        echo "  balance <addr>  - Get account balance"
        echo "  nonce <addr>    - Get account nonce"
        echo "  tx <hash>       - Get transaction by hash"
        echo "  mempool         - Get mempool status"
        echo "  send <from> <to> <value> [nonce] - Submit transaction"
        echo ""
        echo "Environment:"
        echo "  RPC_URL - RPC endpoint (default: http://localhost:8545)"
        echo ""
        echo "Examples:"
        echo "  $0 status"
        echo "  $0 block 0"
        echo "  $0 balance 0x1234..."
        echo "  RPC_URL=http://localhost:8546 $0 status"
        ;;
esac
