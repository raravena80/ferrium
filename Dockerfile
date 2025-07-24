# Build stage
FROM --platform=$BUILDPLATFORM rust:1.82-slim AS builder

# Docker buildx arguments for cross-compilation
ARG TARGETPLATFORM
ARG BUILDPLATFORM
ARG TARGETOS
ARG TARGETARCH

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    protobuf-compiler \
    libclang-dev \
    clang \
    gcc \
    && rm -rf /var/lib/apt/lists/*

# Set up cross-compilation for ARM64 if needed
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    apt-get update && apt-get install -y gcc-aarch64-linux-gnu && \
    rustup target add aarch64-unknown-linux-gnu && \
    rm -rf /var/lib/apt/lists/*; \
fi

# Set up cross-compilation for AMD64 if needed (when building on ARM)
RUN if [ "$TARGETARCH" = "amd64" ]; then \
    rustup target add x86_64-unknown-linux-gnu; \
fi

WORKDIR /usr/src/ferrium

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./

# Copy build configuration and proto files
COPY build.rs ./
COPY proto ./proto

# Copy source code
COPY src ./src
COPY tests ./tests

# Configure environment for cross-compilation
ENV PKG_CONFIG_ALLOW_CROSS=1

# Set target-specific environment variables
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    echo 'export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc' >> ~/.bashrc && \
    echo 'export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc' >> ~/.bashrc; \
fi

# Build the application for the target architecture
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    cargo build --release --target=aarch64-unknown-linux-gnu; \
elif [ "$TARGETARCH" = "amd64" ]; then \
    cargo build --release --target=x86_64-unknown-linux-gnu; \
else \
    cargo build --release; \
fi

# Copy the correct binary based on target architecture
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    cp target/aarch64-unknown-linux-gnu/release/ferrium-server /usr/local/bin/ferrium-server; \
elif [ "$TARGETARCH" = "amd64" ]; then \
    cp target/x86_64-unknown-linux-gnu/release/ferrium-server /usr/local/bin/ferrium-server; \
else \
    cp target/release/ferrium-server /usr/local/bin/ferrium-server; \
fi

# Runtime stage  
FROM --platform=$TARGETPLATFORM debian:bookworm-slim

# Docker buildx arguments for runtime stage
ARG TARGETPLATFORM
ARG TARGETARCH

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

# Copy the binary (from the builder stage where we put it in /usr/local/bin)
COPY --from=builder /usr/local/bin/ferrium-server /usr/local/bin/ferrium-server

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
