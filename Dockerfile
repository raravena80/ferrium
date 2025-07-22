# Build stage
FROM rust:1.75-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/ferrite

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY tests ./tests

# Build the application
RUN cargo build --release

# Runtime stage  
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r ferrite && useradd -r -g ferrite ferrite

# Create directories
RUN mkdir -p /etc/ferrite /var/lib/ferrite && \
    chown -R ferrite:ferrite /var/lib/ferrite

# Copy the binary
COPY --from=builder /usr/src/ferrite/target/release/ferrite-server /usr/local/bin/ferrite-server

# Set permissions
RUN chmod +x /usr/local/bin/ferrite-server

# Switch to non-root user
USER ferrite

# Default data directory
VOLUME ["/var/lib/ferrite"]

# Default ports (HTTP API, gRPC, Raft)
EXPOSE 8001 9001

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8001/health || exit 1

# Default command
CMD ["ferrite-server", "--data-dir", "/var/lib/ferrite", "--http-addr", "0.0.0.0:8001", "--grpc-addr", "0.0.0.0:9001"] 