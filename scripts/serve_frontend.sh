#!/bin/bash

# Serve the blockchain explorer frontend
# This script starts a simple HTTP server for the frontend

PORT=${PORT:-3000}
FRONTEND_DIR="$(dirname "$0")/../frontend"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}======================================${NC}"
echo -e "${BLUE}  P2P PBFT Blockchain Explorer${NC}"
echo -e "${BLUE}======================================${NC}"
echo ""

# Check if frontend directory exists
if [ ! -d "$FRONTEND_DIR" ]; then
    echo -e "${RED}Frontend directory not found: $FRONTEND_DIR${NC}"
    exit 1
fi

cd "$FRONTEND_DIR"

echo -e "${GREEN}Starting frontend server...${NC}"
echo ""
echo "  URL: http://localhost:$PORT"
echo "  Directory: $FRONTEND_DIR"
echo ""
echo -e "${YELLOW}Make sure the blockchain is running:${NC}"
echo "  ./scripts/run_blockchain.sh start"
echo ""
echo -e "${YELLOW}Press Ctrl+C to stop${NC}"
echo ""

# Try different methods to serve files
if command -v python3 &> /dev/null; then
    echo "Using Python 3 HTTP server..."
    python3 -m http.server $PORT
elif command -v python &> /dev/null; then
    echo "Using Python 2 HTTP server..."
    python -m SimpleHTTPServer $PORT
elif command -v npx &> /dev/null; then
    echo "Using npx serve..."
    npx serve -l $PORT
elif command -v php &> /dev/null; then
    echo "Using PHP built-in server..."
    php -S localhost:$PORT
else
    echo -e "${RED}No suitable HTTP server found!${NC}"
    echo ""
    echo "Please install one of the following:"
    echo "  - Python 3: brew install python"
    echo "  - Node.js: brew install node"
    echo "  - PHP: brew install php"
    echo ""
    echo "Or manually serve the frontend directory:"
    echo "  $FRONTEND_DIR"
    exit 1
fi
