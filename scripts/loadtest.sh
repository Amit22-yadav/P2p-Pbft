#!/bin/bash

# P2P PBFT Blockchain - Load Test / Parallel Transaction Script
# Generate accounts, fund them, and send parallel transactions

set -e

# Change to project root directory (parent of scripts/)
cd "$(dirname "$0")/.." || exit 1

# Configuration
RPC_URL=${RPC_URL:-"http://localhost:8545"}
WALLET="./target/release/wallet"
BINARY="./target/release/p2p-pbft"
ACCOUNTS_FILE="./data/loadtest_accounts.json"

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

# Check if binaries exist
check_binaries() {
    if [ ! -f "$WALLET" ]; then
        echo -e "${RED}Error: Wallet binary not found at $WALLET${NC}"
        echo "Please build first: cargo build --release"
        exit 1
    fi
    if [ ! -f "$BINARY" ]; then
        echo -e "${RED}Error: p2p-pbft binary not found at $BINARY${NC}"
        echo "Please build first: cargo build --release"
        exit 1
    fi
}

# Get address from seed
get_address() {
    local seed=$1
    $BINARY keygen --seed "$seed" 2>/dev/null | grep "Address:" | awk '{print $2}'
}

# Get nonce (returns decimal)
get_nonce() {
    local addr=$1
    local result=$(rpc_call "eth_getTransactionCount" "\"$addr\"" | jq -r '.result // "0x0"')
    printf "%d" "$result"
}

# Get balance
get_balance() {
    local addr=$1
    rpc_call "eth_getBalance" "\"$addr\"" | jq -r '.result // "0x0"'
}

# Validator seed (index byte at position 0)
validator_seed() {
    local index=$1
    printf '%02x' $index
    printf '%062d' 0
}

# Generate a random 32-byte seed
generate_seed() {
    # Use index-based deterministic seed for reproducibility
    local index=$1
    # Seed format: 0xDEAD + index (4 bytes) + padding
    printf 'dead%04x' $index
    printf '%056d' 0
}

# ==================== STEP 1: Generate Accounts ====================
generate_accounts() {
    local count=$1

    echo -e "${BLUE}Step 1: Generating $count accounts...${NC}"

    mkdir -p "$(dirname "$ACCOUNTS_FILE")"

    # Start JSON array
    echo "[" > "$ACCOUNTS_FILE"

    for ((i=0; i<count; i++)); do
        seed=$(generate_seed $i)
        addr=$(get_address "$seed")

        # Add comma for all but last entry
        if [ $i -gt 0 ]; then
            echo "," >> "$ACCOUNTS_FILE"
        fi

        echo "  {\"index\": $i, \"seed\": \"$seed\", \"address\": \"$addr\"}" >> "$ACCOUNTS_FILE"

        # Progress indicator
        if [ $((i % 10)) -eq 0 ]; then
            echo -ne "\r  Generated: $i / $count"
        fi
    done

    echo "" >> "$ACCOUNTS_FILE"
    echo "]" >> "$ACCOUNTS_FILE"

    echo -e "\r  Generated: $count / $count"
    echo -e "${GREEN}  Accounts saved to: $ACCOUNTS_FILE${NC}"
}

# ==================== STEP 2: Fund Accounts ====================
fund_accounts() {
    local amount_per_account=${1:-1000}
    local batch_size=${2:-10}  # Send in batches to avoid overwhelming the network

    if [ ! -f "$ACCOUNTS_FILE" ]; then
        echo -e "${RED}Error: No accounts file found. Run 'generate' first.${NC}"
        exit 1
    fi

    local count=$(jq length "$ACCOUNTS_FILE")
    echo -e "${BLUE}Step 2: Funding $count accounts with $amount_per_account each...${NC}"

    # We'll use all 4 validators to fund accounts in parallel
    local validators=4
    local per_validator=$((count / validators))

    echo "  Using $validators validators to fund accounts"
    echo "  Each validator funds ~$per_validator accounts"
    echo "  Batch size: $batch_size (transactions per batch)"
    echo ""

    # Track nonces for each validator
    declare -a v_nonces
    declare -a v_seeds
    declare -a v_addrs

    # Initialize validator info
    for ((v=0; v<validators; v++)); do
        v_seeds[$v]=$(validator_seed $v)
        v_addrs[$v]=$(get_address "${v_seeds[$v]}")
        v_nonces[$v]=$(get_nonce "${v_addrs[$v]}")
        echo "  Validator $v: ${v_addrs[$v]} (nonce: ${v_nonces[$v]})"
    done
    echo ""

    local funded=0
    local batch_count=0

    # Process in batches - each validator funds accounts in round-robin
    while [ $funded -lt $count ]; do
        # Send a batch from each validator
        for ((v=0; v<validators; v++)); do
            local account_idx=$((funded + v))
            if [ $account_idx -ge $count ]; then
                continue
            fi

            local to_addr=$(jq -r ".[$account_idx].address" "$ACCOUNTS_FILE")
            local v_seed=${v_seeds[$v]}
            local v_nonce=${v_nonces[$v]}

            # Send transaction in background
            $WALLET send --seed "$v_seed" --to "$to_addr" --value "$amount_per_account" --nonce "$v_nonce" --rpc "$RPC_URL" > /dev/null 2>&1 &

            v_nonces[$v]=$((v_nonce + 1))
        done

        # Wait for this batch to complete
        wait

        funded=$((funded + validators))
        if [ $funded -gt $count ]; then
            funded=$count
        fi

        batch_count=$((batch_count + 1))
        echo -ne "\r  Funded: $funded / $count (batch $batch_count)"

        # Small delay between batches to let transactions propagate
        if [ $funded -lt $count ]; then
            sleep 0.1
        fi
    done

    echo -e "\r  Funded: $funded / $count              "
    echo ""
    echo -e "${GREEN}  All funding transactions submitted!${NC}"
    echo -e "${YELLOW}  Wait for blocks to confirm before sending more transactions.${NC}"
}

# ==================== STEP 3: Parallel Transactions ====================
run_parallel_transactions() {
    local parallel_count=${1:-10}
    local amount=${2:-10}

    if [ ! -f "$ACCOUNTS_FILE" ]; then
        echo -e "${RED}Error: No accounts file found. Run 'generate' first.${NC}"
        exit 1
    fi

    local total_accounts=$(jq length "$ACCOUNTS_FILE")

    if [ $parallel_count -gt $total_accounts ]; then
        echo -e "${RED}Error: Cannot send $parallel_count parallel txs with only $total_accounts accounts${NC}"
        exit 1
    fi

    echo -e "${BLUE}Step 3: Sending $parallel_count parallel transactions...${NC}"
    echo "  Amount per tx: $amount"
    echo ""

    # Get current nonces for all accounts
    echo -e "${YELLOW}  Fetching nonces...${NC}"
    declare -a nonces
    for ((i=0; i<parallel_count; i++)); do
        local addr=$(jq -r ".[$i].address" "$ACCOUNTS_FILE")
        nonces[$i]=$(get_nonce "$addr")
    done

    # Send parallel transactions
    # Each account i sends to account (i+1) % parallel_count
    echo -e "${YELLOW}  Sending transactions...${NC}"

    local start_time=$(date +%s%N)

    for ((i=0; i<parallel_count; i++)); do
        local from_seed=$(jq -r ".[$i].seed" "$ACCOUNTS_FILE")
        local to_idx=$(( (i + 1) % parallel_count ))
        local to_addr=$(jq -r ".[$to_idx].address" "$ACCOUNTS_FILE")
        local nonce=${nonces[$i]}

        # Send in background
        $WALLET send --seed "$from_seed" --to "$to_addr" --value "$amount" --nonce "$nonce" --rpc "$RPC_URL" > /dev/null 2>&1 &

        echo -ne "\r  Sent: $((i + 1)) / $parallel_count"
    done

    # Wait for all to complete
    wait

    local end_time=$(date +%s%N)
    local elapsed=$(( (end_time - start_time) / 1000000 ))

    echo -e "\r  Sent: $parallel_count / $parallel_count    "
    echo ""
    echo -e "${GREEN}  All $parallel_count transactions submitted in ${elapsed}ms${NC}"

    # Check mempool
    echo ""
    echo -e "${BLUE}  Mempool status:${NC}"
    rpc_call "pbft_getMempool" "" | jq '.result | {total: .total, unique_senders: .unique_senders}'
}

# ==================== STEP 4: Continuous Load ====================
run_continuous_load() {
    local parallel_count=${1:-10}
    local rounds=${2:-10}
    local amount=${3:-1}

    if [ ! -f "$ACCOUNTS_FILE" ]; then
        echo -e "${RED}Error: No accounts file found. Run 'generate' first.${NC}"
        exit 1
    fi

    local total_accounts=$(jq length "$ACCOUNTS_FILE")

    if [ $parallel_count -gt $total_accounts ]; then
        parallel_count=$total_accounts
        echo -e "${YELLOW}Warning: Limiting to $parallel_count parallel txs (available accounts)${NC}"
    fi

    echo -e "${CYAN}======================================${NC}"
    echo -e "${CYAN}  Continuous Load Test${NC}"
    echo -e "${CYAN}======================================${NC}"
    echo ""
    echo "  Parallel transactions: $parallel_count"
    echo "  Rounds: $rounds"
    echo "  Amount per tx: $amount"
    echo "  Total transactions: $((parallel_count * rounds))"
    echo ""

    # Get initial nonces
    echo -e "${YELLOW}Fetching initial nonces...${NC}"
    declare -a nonces
    for ((i=0; i<parallel_count; i++)); do
        local addr=$(jq -r ".[$i].address" "$ACCOUNTS_FILE")
        nonces[$i]=$(get_nonce "$addr")
    done
    echo ""

    local total_sent=0
    local start_time=$(date +%s)

    for ((round=1; round<=rounds; round++)); do
        echo -e "${BLUE}Round $round / $rounds${NC}"

        # Send parallel transactions for this round
        for ((i=0; i<parallel_count; i++)); do
            local from_seed=$(jq -r ".[$i].seed" "$ACCOUNTS_FILE")
            local to_idx=$(( (i + round) % parallel_count ))
            local to_addr=$(jq -r ".[$to_idx].address" "$ACCOUNTS_FILE")
            local nonce=${nonces[$i]}

            # Send in background
            $WALLET send --seed "$from_seed" --to "$to_addr" --value "$amount" --nonce "$nonce" --rpc "$RPC_URL" > /dev/null 2>&1 &

            # Increment nonce for next round
            nonces[$i]=$((nonce + 1))
        done

        # Wait for this round to complete
        wait

        total_sent=$((total_sent + parallel_count))
        echo -e "  Sent $parallel_count txs (total: $total_sent)"

        # Brief pause between rounds
        sleep 0.5
    done

    local end_time=$(date +%s)
    local elapsed=$((end_time - start_time))
    local tps=0
    if [ $elapsed -gt 0 ]; then
        tps=$((total_sent / elapsed))
    fi

    echo ""
    echo -e "${GREEN}======================================${NC}"
    echo -e "${GREEN}  Load Test Complete${NC}"
    echo -e "${GREEN}======================================${NC}"
    echo "  Total transactions sent: $total_sent"
    echo "  Time elapsed: ${elapsed}s"
    echo "  Submission rate: ~${tps} tx/s"
    echo ""

    # Final mempool status
    echo -e "${BLUE}Mempool status:${NC}"
    rpc_call "pbft_getMempool" "" | jq '.result'
}

# ==================== Show Accounts ====================
show_accounts() {
    if [ ! -f "$ACCOUNTS_FILE" ]; then
        echo -e "${RED}Error: No accounts file found. Run 'generate' first.${NC}"
        exit 1
    fi

    local count=$(jq length "$ACCOUNTS_FILE")
    echo -e "${BLUE}Loaded accounts ($count total):${NC}"
    echo ""

    local show_count=${1:-10}
    if [ $show_count -gt $count ]; then
        show_count=$count
    fi

    for ((i=0; i<show_count; i++)); do
        local addr=$(jq -r ".[$i].address" "$ACCOUNTS_FILE")
        local balance=$(get_balance "$addr")
        local nonce=$(get_nonce "$addr")
        echo "  [$i] $addr"
        echo "      Balance: $balance, Nonce: $nonce"
    done

    if [ $show_count -lt $count ]; then
        echo ""
        echo "  ... and $((count - show_count)) more"
    fi
}

# ==================== Check Balances Summary ====================
check_balances() {
    if [ ! -f "$ACCOUNTS_FILE" ]; then
        echo -e "${RED}Error: No accounts file found. Run 'generate' first.${NC}"
        exit 1
    fi

    local count=$(jq length "$ACCOUNTS_FILE")
    echo -e "${BLUE}Checking balances for $count accounts...${NC}"
    echo ""

    local funded=0
    local unfunded=0
    local total_balance=0

    for ((i=0; i<count; i++)); do
        local addr=$(jq -r ".[$i].address" "$ACCOUNTS_FILE")
        local balance_hex=$(get_balance "$addr")
        local balance=$((balance_hex))

        if [ $balance -gt 0 ]; then
            funded=$((funded + 1))
            total_balance=$((total_balance + balance))
        else
            unfunded=$((unfunded + 1))
        fi

        echo -ne "\r  Checked: $((i + 1)) / $count"
    done

    echo -e "\r  Checked: $count / $count    "
    echo ""
    echo -e "${GREEN}Summary:${NC}"
    echo "  Funded accounts:   $funded"
    echo "  Unfunded accounts: $unfunded"
    echo "  Total balance:     $total_balance"
}

# ==================== Quick Full Test ====================
run_full_test() {
    local num_accounts=${1:-20}
    local parallel=${2:-10}
    local rounds=${3:-5}

    echo -e "${CYAN}╔════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║     P2P PBFT Load Test                 ║${NC}"
    echo -e "${CYAN}╚════════════════════════════════════════╝${NC}"
    echo ""
    echo "  Accounts to create: $num_accounts"
    echo "  Parallel transactions: $parallel"
    echo "  Rounds: $rounds"
    echo ""

    # Check node status
    echo -e "${BLUE}Checking node status...${NC}"
    local status=$(rpc_call "pbft_getStatus" "")
    if ! echo "$status" | jq -e '.result' > /dev/null 2>&1; then
        echo -e "${RED}Error: Node is not running. Start the network first.${NC}"
        exit 1
    fi
    echo -e "${GREEN}Node is online!${NC}"
    echo ""

    # Step 1: Generate accounts
    generate_accounts $num_accounts
    echo ""

    # Step 2: Fund accounts
    fund_accounts 10000
    echo ""

    # Wait for funding to be confirmed
    echo -e "${YELLOW}Waiting 15 seconds for funding transactions to be confirmed...${NC}"
    for i in {1..15}; do
        echo -ne "\r  Waiting: $i / 15 seconds"
        sleep 1
    done
    echo ""
    echo ""

    # Step 3: Run continuous load
    run_continuous_load $parallel $rounds 10
}

# ==================== Main ====================
case "${1:-help}" in
    gen|generate)
        check_binaries
        count=${2:-20}
        generate_accounts $count
        ;;

    fund)
        check_binaries
        amount=${2:-1000}
        fund_accounts $amount
        ;;

    parallel|p)
        check_binaries
        count=${2:-10}
        amount=${3:-10}
        run_parallel_transactions $count $amount
        ;;

    load|continuous)
        check_binaries
        parallel=${2:-10}
        rounds=${3:-10}
        amount=${4:-1}
        run_continuous_load $parallel $rounds $amount
        ;;

    show|list)
        count=${2:-10}
        show_accounts $count
        ;;

    balances|bal)
        check_balances
        ;;

    full|test)
        check_binaries
        accounts=${2:-20}
        parallel=${3:-10}
        rounds=${4:-5}
        run_full_test $accounts $parallel $rounds
        ;;

    clean)
        echo -e "${YELLOW}Removing accounts file...${NC}"
        rm -f "$ACCOUNTS_FILE"
        echo -e "${GREEN}Done.${NC}"
        ;;

    status|st)
        echo -e "${BLUE}Node Status:${NC}"
        rpc_call "pbft_getStatus" "" | jq '.result'
        echo ""
        echo -e "${BLUE}Mempool:${NC}"
        rpc_call "pbft_getMempool" "" | jq '.result'
        ;;

    help|--help|-h|*)
        echo -e "${CYAN}P2P PBFT Blockchain - Load Test Script${NC}"
        echo ""
        echo "Usage: $0 <command> [args...]"
        echo ""
        echo -e "${GREEN}Commands:${NC}"
        echo ""
        echo "  ${YELLOW}full, test${NC} [accounts] [parallel] [rounds]"
        echo "      Run complete load test (default: 20 accounts, 10 parallel, 5 rounds)"
        echo "      Example: $0 full 100 50 10"
        echo ""
        echo "  ${YELLOW}gen, generate${NC} [count]"
        echo "      Generate test accounts (default: 20)"
        echo "      Example: $0 gen 100"
        echo ""
        echo "  ${YELLOW}fund${NC} [amount]"
        echo "      Fund all generated accounts (default: 1000 per account)"
        echo "      Example: $0 fund 5000"
        echo ""
        echo "  ${YELLOW}parallel, p${NC} [count] [amount]"
        echo "      Send parallel transactions (default: 10 txs, 10 amount)"
        echo "      Example: $0 p 50 100"
        echo ""
        echo "  ${YELLOW}load, continuous${NC} [parallel] [rounds] [amount]"
        echo "      Continuous load test (default: 10 parallel, 10 rounds, 1 amount)"
        echo "      Example: $0 load 50 20 5"
        echo ""
        echo "  ${YELLOW}show, list${NC} [count]"
        echo "      Show generated accounts with balances"
        echo "      Example: $0 show 20"
        echo ""
        echo "  ${YELLOW}status, st${NC}"
        echo "      Show node and mempool status"
        echo ""
        echo "  ${YELLOW}clean${NC}"
        echo "      Remove generated accounts file"
        echo ""
        echo -e "${GREEN}Quick Start:${NC}"
        echo "  1. Start network:  ./scripts/start.sh"
        echo "  2. Run full test:  $0 full 100 50 10"
        echo ""
        echo -e "${GREEN}Step by Step:${NC}"
        echo "  1. Generate:       $0 gen 100"
        echo "  2. Fund:           $0 fund 10000"
        echo "  3. Wait for block confirmation (~10s)"
        echo "  4. Send parallel:  $0 p 50"
        echo "  5. Or continuous:  $0 load 50 20"
        echo ""
        echo -e "${GREEN}Examples:${NC}"
        echo "  # Quick test with 20 accounts, 10 parallel txs"
        echo "  $0 full"
        echo ""
        echo "  # Large test: 100 accounts, 50 parallel, 20 rounds"
        echo "  $0 full 100 50 20"
        echo ""
        echo "  # Just send 100 parallel transactions"
        echo "  $0 gen 100 && $0 fund && sleep 15 && $0 p 100"
        ;;
esac
