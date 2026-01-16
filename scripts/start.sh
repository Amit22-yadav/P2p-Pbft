#!/bin/bash

# P2P PBFT Blockchain - All-in-One Starter
# Starts both the blockchain network and web explorer with a single command

set -e

# Change to project root directory (parent of scripts/)
cd "$(dirname "$0")/.." || exit 1

# Configuration
NUM_NODES=${NUM_NODES:-4}
FRONTEND_PORT=${FRONTEND_PORT:-3000}

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Cleanup function
cleanup() {
    echo ""
    echo -e "${YELLOW}Shutting down...${NC}"

    # Kill frontend server if running
    if [ -n "$FRONTEND_PID" ] && kill -0 "$FRONTEND_PID" 2>/dev/null; then
        echo "Stopping frontend server..."
        kill "$FRONTEND_PID" 2>/dev/null || true
    fi

    # Stop blockchain nodes
    echo "Stopping blockchain nodes..."
    ./scripts/run_blockchain.sh stop 2>/dev/null || true

    echo -e "${GREEN}Shutdown complete.${NC}"
    exit 0
}

# Set up signal handlers
trap cleanup SIGINT SIGTERM

print_banner() {
    echo -e "${CYAN}"
    echo "  ╔═══════════════════════════════════════════════════╗"
    echo "  ║                                                   ║"
    echo "  ║           P2P PBFT BLOCKCHAIN                     ║"
    echo "  ║                                                   ║"
    echo "  ║   Consensus Network + Web Explorer                ║"
    echo "  ║                                                   ║"
    echo "  ╚═══════════════════════════════════════════════════╝"
    echo -e "${NC}"
}

case "${1:-start}" in
    start)
        print_banner

        # Check if already running
        if pgrep -f "p2p-pbft.*--node-id" > /dev/null 2>&1; then
            echo -e "${YELLOW}Blockchain nodes appear to be already running.${NC}"
            echo "Run './scripts/start.sh stop' first, or './scripts/start.sh restart'"
            exit 1
        fi

        # Start blockchain
        echo -e "${BLUE}[1/2] Starting blockchain network (${NUM_NODES} nodes)...${NC}"
        ./scripts/run_blockchain.sh start

        # Wait for nodes to be ready
        echo ""
        echo -e "${BLUE}Waiting for nodes to initialize...${NC}"
        sleep 3

        # Check if blockchain is running
        if ! curl -s http://localhost:8545 -X POST -H "Content-Type: application/json" \
            -d '{"jsonrpc":"2.0","method":"pbft_getStatus","params":[],"id":1}' | grep -q "result"; then
            echo -e "${RED}Warning: Blockchain may not be fully ready yet.${NC}"
        else
            echo -e "${GREEN}Blockchain is running!${NC}"
        fi

        echo ""
        echo -e "${BLUE}[2/2] Starting web explorer on port ${FRONTEND_PORT}...${NC}"
        echo ""

        # Start frontend in background using subshell to maintain correct directory
        FRONTEND_DIR="$(pwd)/frontend"
        FRONTEND_LOG="$(pwd)/logs/frontend.log"
        mkdir -p "$(pwd)/logs"

        # Find an HTTP server and start it
        if command -v python3 &> /dev/null; then
            (cd "$FRONTEND_DIR" && exec python3 -m http.server $FRONTEND_PORT) > "$FRONTEND_LOG" 2>&1 &
            FRONTEND_PID=$!
            HTTP_SERVER="Python"
        elif command -v python &> /dev/null; then
            (cd "$FRONTEND_DIR" && exec python -m SimpleHTTPServer $FRONTEND_PORT) > "$FRONTEND_LOG" 2>&1 &
            FRONTEND_PID=$!
            HTTP_SERVER="Python"
        elif command -v npx &> /dev/null; then
            (cd "$FRONTEND_DIR" && exec npx serve -l $FRONTEND_PORT) > "$FRONTEND_LOG" 2>&1 &
            FRONTEND_PID=$!
            HTTP_SERVER="npx serve"
        elif command -v php &> /dev/null; then
            (cd "$FRONTEND_DIR" && exec php -S localhost:$FRONTEND_PORT) > "$FRONTEND_LOG" 2>&1 &
            FRONTEND_PID=$!
            HTTP_SERVER="PHP"
        else
            echo -e "${RED}No HTTP server found. Frontend will not be available.${NC}"
            echo "Install Python 3, Node.js, or PHP to enable the web explorer."
            FRONTEND_PID=""
            HTTP_SERVER=""
        fi

        # Wait a moment for frontend to start
        sleep 1

        echo ""
        echo -e "${GREEN}════════════════════════════════════════════════════${NC}"
        echo -e "${GREEN}  All services started successfully!${NC}"
        echo -e "${GREEN}════════════════════════════════════════════════════${NC}"
        echo ""
        echo -e "  ${CYAN}Web Explorer:${NC}     http://localhost:${FRONTEND_PORT}"
        echo -e "  ${CYAN}RPC Endpoint:${NC}     http://localhost:8545"
        echo -e "  ${CYAN}Nodes:${NC}            ${NUM_NODES} validators running"
        echo ""
        echo -e "  ${YELLOW}Quick Commands:${NC}"
        echo "    ./scripts/client.sh status      # Check node status"
        echo "    ./scripts/client.sh validators  # List validators"
        echo "    ./scripts/client.sh demo        # Run demo"
        echo ""
        echo -e "  ${YELLOW}Press Ctrl+C to stop all services${NC}"
        echo ""

        # Keep script running and wait for signals
        while true; do
            # Check if blockchain is still running
            if ! pgrep -f "p2p-pbft.*--node-id" > /dev/null 2>&1; then
                echo -e "${RED}Blockchain nodes stopped unexpectedly.${NC}"
                cleanup
            fi

            # Check if frontend is still running (if it was started)
            if [ -n "$FRONTEND_PID" ] && ! kill -0 "$FRONTEND_PID" 2>/dev/null; then
                echo -e "${YELLOW}Frontend server stopped. Restarting...${NC}"
                if [ "$HTTP_SERVER" = "Python" ]; then
                    (cd "$FRONTEND_DIR" && exec python3 -m http.server $FRONTEND_PORT) > "$FRONTEND_LOG" 2>&1 &
                    FRONTEND_PID=$!
                fi
            fi

            sleep 5
        done
        ;;

    stop)
        echo -e "${BLUE}Stopping all services...${NC}"

        # Stop frontend servers
        pkill -f "python.*http.server.*$FRONTEND_PORT" 2>/dev/null || true
        pkill -f "SimpleHTTPServer.*$FRONTEND_PORT" 2>/dev/null || true
        pkill -f "serve.*-l.*$FRONTEND_PORT" 2>/dev/null || true
        pkill -f "php.*-S.*localhost:$FRONTEND_PORT" 2>/dev/null || true

        # Stop blockchain
        ./scripts/run_blockchain.sh stop

        echo -e "${GREEN}All services stopped.${NC}"
        ;;

    restart)
        $0 stop
        sleep 2
        $0 start
        ;;

    status)
        echo -e "${BLUE}Service Status${NC}"
        echo ""

        # Check blockchain
        echo -n "Blockchain: "
        if pgrep -f "p2p-pbft.*--node-id" > /dev/null 2>&1; then
            echo -e "${GREEN}Running${NC}"
            ./scripts/run_blockchain.sh status 2>/dev/null || true
        else
            echo -e "${RED}Stopped${NC}"
        fi

        echo ""

        # Check frontend
        echo -n "Frontend:   "
        if curl -s "http://localhost:$FRONTEND_PORT" > /dev/null 2>&1; then
            echo -e "${GREEN}Running${NC} (http://localhost:$FRONTEND_PORT)"
        else
            echo -e "${RED}Stopped${NC}"
        fi
        ;;

    help|--help|-h)
        echo "P2P PBFT Blockchain - All-in-One Starter"
        echo ""
        echo "Usage: $0 [command]"
        echo ""
        echo "Commands:"
        echo "  start     Start blockchain network and web explorer (default)"
        echo "  stop      Stop all services"
        echo "  restart   Restart all services"
        echo "  status    Check service status"
        echo "  help      Show this help message"
        echo ""
        echo "Environment Variables:"
        echo "  NUM_NODES       Number of validator nodes (default: 4)"
        echo "  FRONTEND_PORT   Web explorer port (default: 3000)"
        echo ""
        echo "Examples:"
        echo "  $0                    # Start everything"
        echo "  $0 start              # Same as above"
        echo "  $0 stop               # Stop everything"
        echo "  FRONTEND_PORT=8080 $0 # Use custom frontend port"
        ;;

    *)
        echo -e "${RED}Unknown command: $1${NC}"
        echo "Run '$0 help' for usage information."
        exit 1
        ;;
esac
