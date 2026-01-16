#!/bin/bash
set -e

# P2P PBFT Blockchain Docker Entrypoint

# Default values
NODE_INDEX=${NODE_INDEX:-0}
NUM_NODES=${NUM_NODES:-4}
P2P_PORT=${P2P_PORT:-9000}
RPC_PORT=${RPC_PORT:-8545}
DATA_DIR=${DATA_DIR:-/app/data}
CHAIN_ID=${CHAIN_ID:-pbft-chain}
INITIAL_BALANCE=${INITIAL_BALANCE:-1000000}

case "$1" in
    blockchain|node)
        echo "Starting PBFT blockchain node..."
        echo "  Node Index: $NODE_INDEX"
        echo "  P2P Port: $P2P_PORT"
        echo "  RPC Port: $RPC_PORT"
        echo "  Data Dir: $DATA_DIR"
        echo "  Chain ID: $CHAIN_ID"
        echo "  Total Nodes: $NUM_NODES"

        # Build bootstrap arguments from environment
        BOOTSTRAP_ARGS=""
        if [ -n "$BOOTSTRAP_NODES" ]; then
            for node in $BOOTSTRAP_NODES; do
                BOOTSTRAP_ARGS="$BOOTSTRAP_ARGS --bootstrap $node"
            done
        fi

        exec /app/p2p-pbft blockchain \
            --port "$P2P_PORT" \
            --rpc-port "$RPC_PORT" \
            --data-dir "$DATA_DIR" \
            --chain-id "$CHAIN_ID" \
            --nodes "$NUM_NODES" \
            --index "$NODE_INDEX" \
            --initial-balance "$INITIAL_BALANCE" \
            $BOOTSTRAP_ARGS
        ;;

    wallet)
        shift
        exec /app/wallet "$@"
        ;;

    keygen)
        shift
        exec /app/p2p-pbft keygen "$@"
        ;;

    frontend|explorer)
        echo "Starting frontend server on port 3000..."
        cd /app/frontend
        exec python3 -m http.server 3000
        ;;

    shell|bash)
        exec /bin/bash
        ;;

    *)
        # If command doesn't match, pass it directly to p2p-pbft
        exec /app/p2p-pbft "$@"
        ;;
esac
