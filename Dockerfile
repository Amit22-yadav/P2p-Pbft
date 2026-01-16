# Build stage
FROM rust:1.75-bookworm as builder

# Install dependencies for RocksDB
RUN apt-get update && apt-get install -y \
    clang \
    libclang-dev \
    llvm-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy only dependency files first to cache dependencies
COPY Cargo.toml Cargo.lock* ./

# Create dummy source files to build dependencies
RUN mkdir -p src/bin && \
    echo "fn main() {}" > src/main.rs && \
    echo "fn main() {}" > src/bin/wallet.rs && \
    echo "" > src/lib.rs

# Build dependencies (this layer will be cached)
RUN cargo build --release || true

# Remove dummy source files
RUN rm -rf src

# Copy actual source code
COPY src ./src

# Build the actual binaries
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libgcc-s1 \
    curl \
    python3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /app/target/release/p2p-pbft /app/p2p-pbft
COPY --from=builder /app/target/release/wallet /app/wallet

# Copy frontend for explorer
COPY frontend /app/frontend

# Copy scripts
COPY scripts /app/scripts

# Create data directory
RUN mkdir -p /app/data /app/logs

# Expose ports
# P2P port (base)
EXPOSE 9000
# RPC port (base)
EXPOSE 8545
# Frontend port
EXPOSE 3000

# Default environment variables
ENV RUST_LOG=info
ENV DATA_DIR=/app/data
ENV CHAIN_ID=pbft-chain

# Entrypoint script
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh

ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD ["blockchain"]
