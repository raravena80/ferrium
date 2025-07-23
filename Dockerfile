# Build stage
FROM rust:1.82-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    protobuf-compiler \
    libclang-dev \
    clang \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/ferrium

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./

# Copy build configuration and proto files
COPY build.rs ./
COPY proto ./proto

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
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r ferrium && useradd -r -g ferrium ferrium

# Create directories
RUN mkdir -p /etc/ferrium /var/lib/ferrium && \
    chown -R ferrium:ferrium /var/lib/ferrium

# Copy the binary
COPY --from=builder /usr/src/ferrium/target/release/ferrium-server /usr/local/bin/ferrium-server

# Set permissions
RUN chmod +x /usr/local/bin/ferrium-server

# Switch to non-root user
USER ferrium

# Default data directory
VOLUME ["/var/lib/ferrium"]

# Default ports (HTTP API, gRPC, Raft)
EXPOSE 8001 9001

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8001/health || exit 1

# Default command
CMD ["ferrium-server", "--data-dir", "/var/lib/ferrium", "--http-addr", "0.0.0.0:8001", "--grpc-addr", "0.0.0.0:9001"]
