#!/bin/bash

# P2P PBFT Blockchain Client
# Demonstrates account creation, funding, and transfers

set -e

# Change to project root directory (parent of scripts/)
cd "$(dirname "$0")/.." || exit 1

RPC_URL=${RPC_URL:-"http://localhost:8545"}
BINARY="./target/release/p2p-pbft"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Helper function for RPC calls
rpc_call() {
    local method=$1
    local params=$2
    curl -s "$RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":[$params],\"id\":1}"
}

# Generate a keypair and return address
generate_account() {
    local seed=$1
    if [ -n "$seed" ]; then
        $BINARY keygen --seed "$seed" 2>/dev/null
    else
        $BINARY keygen 2>/dev/null
    fi
}

case "${1:-help}" in
    # ==================== Account Management ====================

    new-account|new)
        echo -e "${BLUE}Creating new account...${NC}"
        echo ""
        generate_account "$2"
        echo ""
        echo -e "${YELLOW}Save your seed/private key securely!${NC}"
        echo -e "${YELLOW}The address is derived from the public key.${NC}"
        ;;

    new-from-seed|seed)
        if [ -z "$2" ]; then
            echo "Usage: $0 new-from-seed <seed-hex>"
            echo ""
            echo "Seed must be 32 bytes (64 hex characters)"
            echo "Example: $0 new-from-seed 0000000000000000000000000000000000000000000000000000000000000001"
            exit 1
        fi
        echo -e "${BLUE}Creating account from seed...${NC}"
        echo ""
        generate_account "$2"
        ;;

    balance|bal)
        address=$2
        if [ -z "$address" ]; then
            echo "Usage: $0 balance <address>"
            exit 1
        fi
        echo -e "${BLUE}Getting balance for $address${NC}"
        result=$(rpc_call "eth_getBalance" "\"$address\"")
        balance=$(echo "$result" | jq -r '.result // "error"')
        if [ "$balance" == "error" ] || [ "$balance" == "null" ]; then
            echo -e "${RED}Error: $(echo "$result" | jq -r '.error.message // "Unknown error"')${NC}"
        else
            echo -e "${GREEN}Balance: $balance${NC}"
        fi
        ;;

    nonce|n)
        address=$2
        if [ -z "$address" ]; then
            echo "Usage: $0 nonce <address>"
            exit 1
        fi
        echo -e "${BLUE}Getting nonce for $address${NC}"
        result=$(rpc_call "eth_getTransactionCount" "\"$address\"")
        nonce=$(echo "$result" | jq -r '.result // "error"')
        if [ "$nonce" == "error" ] || [ "$nonce" == "null" ]; then
            echo -e "${RED}Error: $(echo "$result" | jq -r '.error.message // "Unknown error"')${NC}"
        else
            echo -e "${GREEN}Nonce: $nonce${NC}"
        fi
        ;;

    # ==================== Transactions ====================

    transfer|send)
        seed=$2
        to=$3
        value=$4
        nonce=$5

        if [ -z "$seed" ] || [ -z "$to" ] || [ -z "$value" ]; then
            echo "Usage: $0 transfer <seed> <to-address> <value> [nonce]"
            echo ""
            echo "Example:"
            echo "  $0 transfer 0000...0000 0x5678... 100"
            echo ""
            echo "Note: The seed is a 64-character hex string (32 bytes)."
            echo "      Use 'validators' command to see validator seeds."
            exit 1
        fi

        WALLET="./target/release/wallet"
        if [ ! -f "$WALLET" ]; then
            echo -e "${RED}Wallet binary not found. Run: cargo build --release${NC}"
            exit 1
        fi

        # Get sender address from seed
        from=$($WALLET account --seed "$seed" 2>/dev/null | grep "Address:" | awk '{print $2}')

        # Get nonce if not provided
        if [ -z "$nonce" ]; then
            result=$(rpc_call "eth_getTransactionCount" "\"$from\"")
            nonce_hex=$(echo "$result" | jq -r '.result // "0x0"')
            nonce=$((nonce_hex))
        fi

        echo -e "${BLUE}Creating and submitting signed transfer transaction...${NC}"
        echo "  From:  $from"
        echo "  To:    $to"
        echo "  Value: $value"
        echo "  Nonce: $nonce"
        echo ""

        # Use wallet to send transaction
        $WALLET send --seed "$seed" --to "$to" --value "$value" --nonce "$nonce" --rpc "$RPC_URL"
        ;;

    # ==================== Query ====================

    tx)
        hash=$2
        if [ -z "$hash" ]; then
            echo "Usage: $0 tx <hash>"
            exit 1
        fi
        echo -e "${BLUE}Getting transaction $hash${NC}"
        rpc_call "eth_getTransactionByHash" "\"$hash\"" | jq .
        ;;

    block|b)
        height=${2:-0}
        echo -e "${BLUE}Getting block $height${NC}"
        rpc_call "eth_getBlockByNumber" "\"$height\"" | jq .
        ;;

    status|st)
        echo -e "${BLUE}Getting node status${NC}"
        rpc_call "pbft_getStatus" "" | jq .
        ;;

    mempool|mp)
        echo -e "${BLUE}Getting mempool status${NC}"
        rpc_call "pbft_getMempool" "" | jq .
        ;;

    height|h)
        result=$(rpc_call "eth_blockNumber" "")
        height=$(echo "$result" | jq -r '.result // "error"')
        echo -e "${GREEN}Current height: $height${NC}"
        ;;

    # ==================== Demo ====================

    demo)
        echo -e "${CYAN}======================================${NC}"
        echo -e "${CYAN}  P2P PBFT Blockchain Demo${NC}"
        echo -e "${CYAN}======================================${NC}"
        echo ""

        # Check if nodes are running
        echo -e "${BLUE}1. Checking node status...${NC}"
        status=$(rpc_call "pbft_getStatus" "")
        if echo "$status" | jq -e '.result' > /dev/null 2>&1; then
            echo -e "${GREEN}   Node is online!${NC}"
            echo "$status" | jq '.result'
        else
            echo -e "${RED}   Node is offline. Start the network first:${NC}"
            echo "   ./scripts/run_blockchain.sh start"
            exit 1
        fi
        echo ""

        # Show validator addresses (these are funded in genesis)
        echo -e "${BLUE}2. Validator addresses (funded in genesis):${NC}"
        for i in 0 1 2 3; do
            # Seed format: index byte at position 0, rest zeros (e.g., 0100000...000 for validator 1)
            seed=$(printf '%02x' $i)$(printf '%062d' 0)
            addr=$($BINARY keygen --seed "$seed" 2>/dev/null | grep "Address:" | awk '{print $2}')
            balance=$(rpc_call "eth_getBalance" "\"$addr\"" | jq -r '.result // 0')
            echo "   Validator $i: $addr (balance: $balance)"
        done
        echo ""

        # Get validator 0's address and seed for demo
        VALIDATOR_0_SEED="0000000000000000000000000000000000000000000000000000000000000000"
        VALIDATOR_0_ADDR=$($BINARY keygen --seed "$VALIDATOR_0_SEED" 2>/dev/null | grep "Address:" | awk '{print $2}')

        # Create a new account
        echo -e "${BLUE}3. Creating a new account...${NC}"
        NEW_SEED="deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        new_account_output=$($BINARY keygen --seed "$NEW_SEED" 2>/dev/null)
        NEW_ADDR=$(echo "$new_account_output" | grep "Address:" | awk '{print $2}')
        echo "   New address: $NEW_ADDR"

        initial_balance=$(rpc_call "eth_getBalance" "\"$NEW_ADDR\"" | jq -r '.result // 0')
        echo "   Initial balance: $initial_balance"
        echo ""

        # Transfer funds using signed transaction
        echo -e "${BLUE}4. Transferring 1000 from Validator 0 to new account...${NC}"
        echo "   From: $VALIDATOR_0_ADDR"
        echo "   To:   $NEW_ADDR"
        echo "   Value: 1000"

        WALLET="./target/release/wallet"
        if [ ! -f "$WALLET" ]; then
            echo -e "${RED}Wallet binary not found. Run: cargo build --release${NC}"
            exit 1
        fi

        # Get current nonce for validator 0
        current_nonce_hex=$(rpc_call "eth_getTransactionCount" "\"$VALIDATOR_0_ADDR\"" | jq -r '.result // "0x0"')
        current_nonce=$((current_nonce_hex))
        echo "   Nonce: $current_nonce"

        result=$($WALLET send --seed "$VALIDATOR_0_SEED" --to "$NEW_ADDR" --value 1000 --nonce "$current_nonce" --rpc "$RPC_URL" 2>&1)
        tx_hash=$(echo "$result" | grep -o '"hash": "[^"]*"' | cut -d'"' -f4 || echo "submitted")

        if echo "$result" | grep -q "Transaction submitted"; then
            echo -e "${GREEN}   Transaction submitted successfully${NC}"
            echo "$result" | tail -n 3
        else
            echo -e "${RED}   Error submitting transaction${NC}"
            echo "$result"
        fi
        echo ""

        # Check mempool
        echo -e "${BLUE}5. Checking mempool...${NC}"
        rpc_call "pbft_getMempool" "" | jq '.result'
        echo ""

        # Wait for block
        echo -e "${BLUE}6. Waiting for block production (up to 10 seconds)...${NC}"
        for i in {1..10}; do
            sleep 1
            echo -n "."
            new_balance=$(rpc_call "eth_getBalance" "\"$NEW_ADDR\"" | jq -r '.result // 0')
            if [ "$new_balance" != "0" ] && [ "$new_balance" != "$initial_balance" ]; then
                echo ""
                echo -e "${GREEN}   Block produced!${NC}"
                break
            fi
        done
        echo ""

        # Check final balances
        echo -e "${BLUE}7. Final balances:${NC}"
        v0_balance=$(rpc_call "eth_getBalance" "\"$VALIDATOR_0_ADDR\"" | jq -r '.result // 0')
        new_balance=$(rpc_call "eth_getBalance" "\"$NEW_ADDR\"" | jq -r '.result // 0')
        echo "   Validator 0: $v0_balance"
        echo "   New account: $new_balance"
        echo ""

        # Show latest block
        echo -e "${BLUE}8. Latest block:${NC}"
        height=$(rpc_call "eth_blockNumber" "" | jq -r '.result // 0')
        echo "   Height: $height"
        if [ "$height" != "0" ]; then
            rpc_call "eth_getBlockByNumber" "\"$height\"" | jq '.result | {number: .number, hash: .hash, tx_count: (.transactions | length)}'
        fi
        echo ""

        echo -e "${CYAN}======================================${NC}"
        echo -e "${CYAN}  Demo Complete!${NC}"
        echo -e "${CYAN}======================================${NC}"
        ;;

    # ==================== Validators ====================

    validators|vals)
        echo -e "${BLUE}Validator accounts (deterministic from index):${NC}"
        echo ""
        for i in 0 1 2 3; do
            # Seed format: index byte at position 0, rest zeros (e.g., 0100000...000 for validator 1)
            seed=$(printf '%02x' $i)$(printf '%062d' 0)
            output=$($BINARY keygen --seed "$seed" 2>/dev/null)
            pubkey=$(echo "$output" | grep "Public Key" | awk '{print $4}')
            addr=$(echo "$output" | grep "Address:" | awk '{print $2}')
            balance=$(rpc_call "eth_getBalance" "\"$addr\"" 2>/dev/null | jq -r '.result // "?"')
            nonce=$(rpc_call "eth_getTransactionCount" "\"$addr\"" 2>/dev/null | jq -r '.result // "?"')
            echo -e "${GREEN}Validator $i:${NC}"
            echo "  Node ID: ${pubkey:0:16}..."
            echo "  Address: $addr"
            echo "  Balance: $balance"
            echo "  Nonce:   $nonce"
            echo ""
        done
        ;;

    # ==================== Help ====================

    help|--help|-h|*)
        echo "P2P PBFT Blockchain Client"
        echo ""
        echo "Usage: $0 <command> [args...]"
        echo ""
        echo -e "${CYAN}Account Commands:${NC}"
        echo "  new-account         Create a new random account"
        echo "  new-from-seed <hex> Create account from 32-byte seed"
        echo "  balance <addr>      Get account balance"
        echo "  nonce <addr>        Get account nonce"
        echo "  validators          Show all validator accounts"
        echo ""
        echo -e "${CYAN}Transaction Commands:${NC}"
        echo "  transfer <from> <to> <value> [nonce]  Transfer funds"
        echo "  tx <hash>           Get transaction by hash"
        echo ""
        echo -e "${CYAN}Query Commands:${NC}"
        echo "  status              Get node status"
        echo "  height              Get current block height"
        echo "  block [N]           Get block by number"
        echo "  mempool             Get mempool status"
        echo ""
        echo -e "${CYAN}Demo:${NC}"
        echo "  demo                Run interactive demo"
        echo ""
        echo -e "${CYAN}Environment:${NC}"
        echo "  RPC_URL             RPC endpoint (default: http://localhost:8545)"
        echo ""
        echo -e "${CYAN}Examples:${NC}"
        echo ""
        echo "  # Create a new account"
        echo "  $0 new-account"
        echo ""
        echo "  # Create account from seed (deterministic)"
        echo "  $0 new-from-seed 0000000000000000000000000000000000000000000000000000000000000005"
        echo ""
        echo "  # Check balance"
        echo "  $0 balance 0x7e5f4552091a69125d5dfcb7b8c2659029395bdf"
        echo ""
        echo "  # Transfer funds"
        echo "  $0 transfer 0xSENDER 0xRECEIVER 1000"
        echo ""
        echo "  # Run full demo"
        echo "  $0 demo"
        ;;
esac
