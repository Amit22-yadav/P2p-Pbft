#!/bin/bash

# P2P PBFT Network Runner Script
# This script helps you build and run a PBFT network with multiple nodes

set -e

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_help() {
    echo -e "${BLUE}P2P PBFT Network Runner${NC}"
    echo ""
    echo "Usage: $0 <command> [options]"
    echo ""
    echo "Commands:"
    echo "  build              Build the project"
    echo "  node <index>       Start a single node (index: 0-3 for 4-node network)"
    echo "  network            Start all 4 nodes in separate terminal windows (macOS)"
    echo "  network-bg         Start all 4 nodes in background"
    echo "  stop               Stop all background nodes"
    echo "  status             Show status of running nodes"
    echo "  keygen             Generate a new keypair"
    echo "  clean              Clean build artifacts"
    echo "  help               Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0 build                    # Build the project"
    echo "  $0 node 0                   # Start node 0 (primary)"
    echo "  $0 node 1                   # Start node 1"
    echo "  $0 network                  # Start 4-node network in terminals"
    echo "  $0 network-bg               # Start 4-node network in background"
    echo ""
}

check_rust() {
    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}Error: Rust/Cargo is not installed${NC}"
        echo "Please install Rust from https://rustup.rs/"
        echo "Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi
}

build_project() {
    echo -e "${BLUE}Building P2P PBFT...${NC}"
    check_rust
    cargo build --release
    echo -e "${GREEN}Build complete!${NC}"
}

start_node() {
    local index=$1
    local port=$((8000 + index))
    local bootstrap=""

    if [ "$index" -gt 0 ]; then
        bootstrap="-b 127.0.0.1:8000"
    fi

    echo -e "${GREEN}Starting node $index on port $port${NC}"

    if [ ! -f "target/release/p2p-pbft" ]; then
        echo -e "${YELLOW}Binary not found, building first...${NC}"
        build_project
    fi

    ./target/release/p2p-pbft start -p "$port" -i "$index" -n 4 $bootstrap
}

start_network_terminals() {
    echo -e "${BLUE}Starting 4-node PBFT network in separate terminals...${NC}"

    if [ ! -f "target/release/p2p-pbft" ]; then
        echo -e "${YELLOW}Binary not found, building first...${NC}"
        build_project
    fi

    # Create logs directory
    mkdir -p logs

    # Detect OS and open terminals accordingly
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS - use Terminal.app or iTerm
        for i in 0 1 2 3; do
            port=$((8000 + i))
            bootstrap=""
            if [ $i -gt 0 ]; then
                bootstrap="-b 127.0.0.1:8000"
            fi

            osascript -e "tell application \"Terminal\" to do script \"cd '$PROJECT_DIR' && ./target/release/p2p-pbft start -p $port -i $i -n 4 $bootstrap 2>&1 | tee '$PROJECT_DIR/logs/node$i.log'\""

            # Small delay to ensure node 0 starts first
            if [ $i -eq 0 ]; then
                sleep 2
            else
                sleep 0.5
            fi
        done

        echo -e "${GREEN}Network started in 4 Terminal windows${NC}"
        echo -e "Logs are being saved to: ${YELLOW}logs/${NC}"

    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        # Linux - try gnome-terminal, xterm, or konsole
        if command -v gnome-terminal &> /dev/null; then
            for i in 0 1 2 3; do
                port=$((8000 + i))
                bootstrap=""
                if [ $i -gt 0 ]; then
                    bootstrap="-b 127.0.0.1:8000"
                fi

                gnome-terminal -- bash -c "cd '$PROJECT_DIR' && ./target/release/p2p-pbft start -p $port -i $i -n 4 $bootstrap 2>&1 | tee '$PROJECT_DIR/logs/node$i.log'; exec bash"

                if [ $i -eq 0 ]; then
                    sleep 2
                else
                    sleep 0.5
                fi
            done
        elif command -v xterm &> /dev/null; then
            for i in 0 1 2 3; do
                port=$((8000 + i))
                bootstrap=""
                if [ $i -gt 0 ]; then
                    bootstrap="-b 127.0.0.1:8000"
                fi

                xterm -hold -e "cd '$PROJECT_DIR' && ./target/release/p2p-pbft start -p $port -i $i -n 4 $bootstrap 2>&1 | tee '$PROJECT_DIR/logs/node$i.log'" &

                if [ $i -eq 0 ]; then
                    sleep 2
                else
                    sleep 0.5
                fi
            done
        else
            echo -e "${RED}No supported terminal emulator found. Use 'network-bg' instead.${NC}"
            exit 1
        fi

        echo -e "${GREEN}Network started in separate terminal windows${NC}"
        echo -e "Logs are being saved to: ${YELLOW}logs/${NC}"
    else
        echo -e "${YELLOW}Unsupported OS for terminal mode. Using background mode...${NC}"
        start_network_background
    fi
}

start_network_background() {
    echo -e "${BLUE}Starting 4-node PBFT network in background...${NC}"

    if [ ! -f "target/release/p2p-pbft" ]; then
        echo -e "${YELLOW}Binary not found, building first...${NC}"
        build_project
    fi

    # Create logs directory
    mkdir -p logs

    # Stop any existing nodes
    stop_network 2>/dev/null || true

    # Start nodes
    for i in 0 1 2 3; do
        port=$((8000 + i))
        bootstrap=""
        if [ $i -gt 0 ]; then
            bootstrap="-b 127.0.0.1:8000"
        fi

        echo -e "Starting node $i on port $port..."
        ./target/release/p2p-pbft start -p "$port" -i "$i" -n 4 $bootstrap > "logs/node$i.log" 2>&1 &
        echo $! > "logs/node$i.pid"

        if [ $i -eq 0 ]; then
            sleep 2  # Give primary time to start
        else
            sleep 0.5
        fi
    done

    echo -e "${GREEN}Network started!${NC}"
    echo ""
    echo "View logs:"
    echo "  tail -f logs/node0.log  # Primary node"
    echo "  tail -f logs/node1.log"
    echo "  tail -f logs/node2.log"
    echo "  tail -f logs/node3.log"
    echo ""
    echo "Stop network:"
    echo "  $0 stop"
}

stop_network() {
    echo -e "${BLUE}Stopping PBFT network...${NC}"

    for i in 0 1 2 3; do
        if [ -f "logs/node$i.pid" ]; then
            pid=$(cat "logs/node$i.pid")
            if kill -0 "$pid" 2>/dev/null; then
                kill "$pid" 2>/dev/null || true
                echo "Stopped node $i (PID: $pid)"
            fi
            rm -f "logs/node$i.pid"
        fi
    done

    # Also kill any remaining processes
    pkill -f "p2p-pbft start" 2>/dev/null || true

    echo -e "${GREEN}Network stopped${NC}"
}

show_status() {
    echo -e "${BLUE}PBFT Network Status${NC}"
    echo ""

    running=0
    for i in 0 1 2 3; do
        port=$((8000 + i))
        if [ -f "logs/node$i.pid" ]; then
            pid=$(cat "logs/node$i.pid")
            if kill -0 "$pid" 2>/dev/null; then
                echo -e "  Node $i (port $port): ${GREEN}RUNNING${NC} (PID: $pid)"
                ((running++))
            else
                echo -e "  Node $i (port $port): ${RED}STOPPED${NC}"
            fi
        else
            # Check if process is running without PID file
            if pgrep -f "p2p-pbft start -p $port" > /dev/null 2>&1; then
                echo -e "  Node $i (port $port): ${GREEN}RUNNING${NC}"
                ((running++))
            else
                echo -e "  Node $i (port $port): ${YELLOW}NOT STARTED${NC}"
            fi
        fi
    done

    echo ""
    echo "Running nodes: $running/4"

    if [ $running -ge 3 ]; then
        echo -e "${GREEN}Network has quorum (3+ nodes)${NC}"
    else
        echo -e "${YELLOW}Network does NOT have quorum (need 3+ nodes)${NC}"
    fi
}

generate_key() {
    echo -e "${BLUE}Generating new keypair...${NC}"

    if [ ! -f "target/release/p2p-pbft" ]; then
        echo -e "${YELLOW}Binary not found, building first...${NC}"
        build_project
    fi

    ./target/release/p2p-pbft keygen
}

clean_project() {
    echo -e "${BLUE}Cleaning build artifacts...${NC}"
    cargo clean
    rm -rf logs
    echo -e "${GREEN}Clean complete${NC}"
}

# Main script logic
case "${1:-help}" in
    build)
        build_project
        ;;
    node)
        if [ -z "$2" ]; then
            echo -e "${RED}Error: Please specify node index (0-3)${NC}"
            echo "Usage: $0 node <index>"
            exit 1
        fi
        start_node "$2"
        ;;
    network)
        start_network_terminals
        ;;
    network-bg)
        start_network_background
        ;;
    stop)
        stop_network
        ;;
    status)
        show_status
        ;;
    keygen)
        generate_key
        ;;
    clean)
        clean_project
        ;;
    help|--help|-h)
        print_help
        ;;
    *)
        echo -e "${RED}Unknown command: $1${NC}"
        print_help
        exit 1
        ;;
esac
