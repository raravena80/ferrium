<div align="center">
<picture>
<source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
<source media="(prefers-color-scheme: light)" srcset="docs/assets/logo.svg">
<img src="docs/assets/logo.svg" alt="Ferrium Logo" width="400"/>
</picture>

<br/>

<strong>Enterprise Distributed KV Storage with Raft Consensus</strong>

<br/><br/>

<p>
<a href="#-features">Features</a> •
<a href="#-quick-start">Quick Start</a> •
<a href="#-configuration-system">Configuration</a> •
<a href="#-api-reference">API</a> •
<a href="#-deployment-examples">Deploy</a> •
<a href="#-architecture">Architecture</a>
</p>

<p>
<img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square&logo=rust" alt="Rust"/>
<img src="https://img.shields.io/badge/consensus-Raft-blue?style=flat-square" alt="Raft"/>
<img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-green?style=flat-square" alt="License"/>
<img src="https://img.shields.io/badge/status-Production%20Ready-success?style=flat-square" alt="Status"/>
</p>
</div>

---

Ferrium is a production-ready distributed key-value storage system built in Rust using the [openraft](https://github.com/databendlabs/openraft) consensus library. It provides strong consistency guarantees similar to etcd or Consul, with comprehensive configuration management and dual-protocol support (HTTP + gRPC).

## Features

### Core Distributed System
- **Raft Consensus Protocol**: Built on openraft for strong consistency and fault tolerance
- **Persistent Storage**: RocksDB-based storage with automatic snapshots and log compaction
- **Dynamic Membership**: Add/remove nodes without downtime
- **Leader Election**: Automatic failover with configurable timeouts
- **Linearizable Reads**: Strong consistency guarantees for all operations

### Dual-Protocol APIs
- **HTTP REST API**: Human-friendly RESTful interface for web integration
- **gRPC API**: High-performance binary protocol for service-to-service communication
- **Automatic Protocol Detection**: Choose the best protocol for your use case

### Enterprise Configuration System
- **TOML Configuration Files**: Comprehensive settings management
- **CLI Override Support**: Environment-specific parameter overrides
- **Multiple Config Locations**: Automatic discovery from standard paths
- **Configuration Validation**: Extensive validation with helpful error messages
- **Hot Configuration**: Runtime configuration updates (where applicable)

### Production-Ready Operations
- **Performance Tuning**: Optimized configurations for different workload patterns
- **Security**: TLS/mTLS support with multiple authentication methods
- **Monitoring**: Rich metrics and structured logging
- **Health Checks**: Comprehensive health and readiness endpoints
- **Deployment Ready**: Docker, Kubernetes, and systemd integration examples

## Quick Start

### 1. Build Ferrium

```bash
git clone https://github.com/your-org/ferrium
cd ferrium
cargo build --release
```

### 2. Generate Configuration

```bash
# Generate a default configuration file
./target/release/ferrium-server --generate-config ferrium.toml

# Validate your configuration
./target/release/ferrium-server --config ferrium.toml --validate-config
```

### 3. Single Node Development

```bash
# Use the single-node example configuration
./target/release/ferrium-server --config examples/configs/single-node.toml
```

### 4. Production Cluster

```bash
# Node 1 (Primary)
./target/release/ferrium-server --config examples/configs/cluster-node1.toml

# Node 2 & 3 (update configs with appropriate IDs and addresses)
./target/release/ferrium-server --config node2.toml --id 2 --http-addr 10.0.1.11:8001
```

## Configuration System

Ferrium features a comprehensive configuration system supporting every aspect of the distributed system.

### Quick Configuration Setup

```bash
# List available configuration locations
./target/release/ferrium-server --list-config-paths

# Generate default configuration
./target/release/ferrium-server --generate-config my-config.toml

# Validate configuration before deployment
./target/release/ferrium-server --config my-config.toml --validate-config

# Run with configuration and CLI overrides
./target/release/ferrium-server --config my-config.toml --log-level debug --id 42
```

### Configuration Sections

The configuration covers all operational aspects:

- **️ Node**: ID, addresses, data directory, metadata
- **Network**: Timeouts, retries, compression, message limits
- **Storage**: Compression, compaction, caching, durability settings
- **️ Raft**: Consensus parameters, election timeouts, batch sizes
- **Logging**: Levels, formats, rotation, structured logging
- **Cluster**: Peer discovery, membership, priorities
- **Security**: TLS/mTLS, authentication, certificates

### Example Configurations

- **`examples/configs/single-node.toml`** - Development & testing
- **`examples/configs/cluster-node1.toml`** - Production cluster setup
- **`examples/configs/high-performance.toml`** - Optimized for throughput

**See [CONFIG.md](CONFIG.md) for comprehensive configuration documentation**

## API Reference

Ferrium provides both HTTP and gRPC APIs for maximum flexibility.

### HTTP REST API

#### Key-Value Operations

```bash
# Write a key-value pair
curl -X POST -H "Content-Type: application/json" \
-d '{"Set":{"key":"mykey","value":"myvalue"}}' \
http://127.0.0.1:8001/write

# Read a value
curl -X POST -H "Content-Type: application/json" \
-d '{"key":"mykey"}' \
http://127.0.0.1:8001/read

# Delete a key
curl -X POST -H "Content-Type: application/json" \
-d '{"Delete":{"key":"mykey"}}' \
http://127.0.0.1:8001/write
```

#### Cluster Management

```bash
# Health check
curl http://127.0.0.1:8001/health

# Cluster metrics
curl http://127.0.0.1:8001/metrics

# Initialize cluster
curl -X POST http://127.0.0.1:8001/init

# Check leader status
curl http://127.0.0.1:8001/is-leader
curl http://127.0.0.1:8001/leader
```

#### Membership Operations

```bash
# Add learner node
curl -X POST -H "Content-Type: application/json" \
-d '{"node_id":2,"rpc_addr":"127.0.0.1:8002","api_addr":"127.0.0.1:8002"}' \
http://127.0.0.1:8001/add-learner

# Change cluster membership
curl -X POST -H "Content-Type: application/json" \
-d '[1,2,3]' \
http://127.0.0.1:8001/change-membership
```

### gRPC API

The gRPC API provides the same functionality with better performance for service-to-service communication:

```bash
# Test gRPC API
./target/release/grpc-client-test
```

**Services Available:**
- **KvService**: Key-value operations (Get, Set, Delete)
- **ManagementService**: Cluster management and health
- **RaftService**: Internal Raft consensus operations

## Deployment Examples

### Development (Single Node)

```bash
# Quick start with sensible defaults
./target/release/ferrium-server --config examples/configs/single-node.toml
```

### Docker Deployment

```dockerfile
FROM debian:bullseye-slim
COPY target/release/ferrium-server /usr/local/bin/
COPY production.toml /etc/ferrium/config.toml
EXPOSE 8001 9001
CMD ["ferrium-server", "--config", "/etc/ferrium/config.toml"]
```

### Kubernetes Deployment

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
name: ferrium-config
data:
config.toml: |
[node]
id = 1
http_addr = "0.0.0.0:8001"
grpc_addr = "0.0.0.0:9001"
data_dir = "/data"

[logging]
level = "info"
format = "json"
structured = true
---
apiVersion: apps/v1
kind: StatefulSet
metadata:
name: ferrium
spec:
serviceName: ferrium
replicas: 3
selector:
matchLabels:
app: ferrium
template:
metadata:
labels:
app: ferrium
spec:
containers:
- name: ferrium
image: ferrium:latest
command: ["ferrium-server"]
args: ["--config", "/etc/ferrium/config.toml"]
ports:
- containerPort: 8001
name: http
- containerPort: 9001
name: grpc
volumeMounts:
- name: config
mountPath: /etc/ferrium
- name: data
mountPath: /data
volumes:
- name: config
configMap:
name: ferrium-config
volumeClaimTemplates:
- metadata:
name: data
spec:
accessModes: ["ReadWriteOnce"]
resources:
requests:
storage: 10Gi
```

### Systemd Service

```ini
[Unit]
Description=Ferrium Distributed KV Store
After=network.target

[Service]
Type=simple
User=ferrium
Group=ferrium
ExecStart=/usr/local/bin/ferrium-server --config /etc/ferrium/config.toml
Restart=always
RestartSec=10
KillSignal=SIGTERM
TimeoutStopSec=30

# Security
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/ferrium /var/log/ferrium

[Install]
WantedBy=multi-user.target
```

## Architecture

### Core Components

```
┌─────────────────┐    ┌─────────────────┐
│   HTTP REST     │    │      gRPC       │
│      API        │    │       API       │
└─────────────────┘    └─────────────────┘
         │                       │
         └───────────┬───────────┘
                     │
         ┌─────────────────────────────┐
         │    Management Layer         │
         │  (Cluster & Operations)     │
         └─────────────────────────────┘
                     │
         ┌─────────────────────────────┐
         │       Raft Engine           │
         │    (openraft-based)         │
         └─────────────────────────────┘
                     │
         ┌─────────────────────────────┐
         │    Storage Engine           │
         │   (RocksDB-based)           │
         └─────────────────────────────┘
```

### Key Design Features

1. **Configuration-Driven**: Every aspect configurable via TOML files
2. **Protocol Agnostic**: HTTP for humans, gRPC for services
3. **Performance Optimized**: Configurable caching, batching, compression
4. **Security First**: Built-in TLS, authentication, and authorization
5. **Observability**: Rich metrics, structured logging, health checks
6. **Cloud Native**: Kubernetes-ready with proper resource management

### Solving OpenRaft Complexity

Ferrium addresses common openraft challenges:

- **Sealed Traits**: Comprehensive `TypeConfig` implementation
- **Complex Generics**: Simplified through well-defined type bounds
- **Storage Abstraction**: Clean RocksDB integration with proper error handling
- **Network Layer**: HTTP-based communication with automatic retries
- **Configuration**: All Raft parameters tunable via config files

## Performance & Tuning

### Performance Profiles

**High Throughput Configuration:**
```toml
[storage]
sync_writes = false
write_buffer_size = 512
block_cache_size = 1024

[raft]
max_append_entries = 1000
max_inflight_requests = 50
```

**High Durability Configuration:**
```toml
[storage]
sync_writes = true
enable_wal = true

[raft]
snapshot_policy.enable_auto_snapshot = true
```

**Low Latency Configuration:**
```toml
[raft]
heartbeat_interval = 100
election_timeout_min = 150

[network]
request_timeout = 5000
connect_timeout = 1000
```

### Monitoring

```bash
# Get comprehensive metrics
curl http://127.0.0.1:8001/metrics | jq

# Monitor cluster health
watch -n 1 'curl -s http://127.0.0.1:8001/health | jq'

# Check leader status across nodes
for port in 8001 8002 8003; do
echo "Node $port: $(curl -s http://127.0.0.1:$port/is-leader)"
done
```

## Development & Testing

### Running Tests

```bash
# Unit tests
cargo test

# Integration tests
cargo test --test integration

# End-to-end cluster tests
./scripts/test-cluster.sh
```

### Local Development Cluster

```bash
# Start 3-node cluster for testing
./scripts/dev-cluster.sh start

# Run tests against cluster
./scripts/dev-cluster.sh test

# Stop cluster
./scripts/dev-cluster.sh stop
```

### Code Structure

```
src/
├── bin/
│   ├── main.rs              # Server binary with config system
│   ├── grpc_test.rs         # gRPC API test client
│   └── grpc_client_test.rs  # gRPC integration tests
├── lib.rs                   # Library root
├── config/                  # Configuration system
│   └── mod.rs              # TOML config, validation, CLI
├── storage/                 # Storage layer
│   └── mod.rs              # RocksDB integration
├── network/                 # Network & API layer
│   ├── mod.rs              # HTTP network + management API
│   └── client.rs           # HTTP client library
└── grpc/                   # gRPC implementation
    ├── mod.rs              # Proto definitions
    └── services/           # gRPC service implementations
examples/
├── configs/                 # Example configurations
│   ├── single-node.toml    # Development setup
│   ├── cluster-node1.toml  # Production cluster
│   └── high-performance.toml # Performance-optimized
proto/                       # Protocol buffer definitions
├── kv.proto                # KV service definitions
├── management.proto        # Management service
└── raft.proto             # Raft internal protocols
```

## Troubleshooting

### Common Issues

**"No leader found" errors:**
```bash
# Check cluster status
curl http://127.0.0.1:8001/leader
curl http://127.0.0.1:8001/metrics

# Verify node connectivity
curl http://127.0.0.1:8002/health
curl http://127.0.0.1:8003/health
```

**Configuration errors:**
```bash
# Validate your configuration
ferrium-server --config my-config.toml --validate-config

# Check configuration locations
ferrium-server --list-config-paths
```

**Performance issues:**
```bash
# Use high-performance configuration
ferrium-server --config examples/configs/high-performance.toml

# Monitor metrics for bottlenecks
watch -n 1 'curl -s http://127.0.0.1:8001/metrics | jq .current_leader'
```

### Debug Logging

```bash
# Enable comprehensive debugging
RUST_LOG=ferrium=debug,openraft=debug ferrium-server --config debug.toml

# JSON structured logging for analysis
ferrium-server --config production.toml --log-level debug --format json
```

## Roadmap

### Current Status: Production Ready

- [x] Raft consensus implementation
- [x] Persistent storage with RocksDB
- [x] HTTP + gRPC dual APIs
- [x] Comprehensive configuration system
- [x] Security (TLS/mTLS/Auth)
- [x] Performance tuning capabilities
- [x] Production deployment examples

### Future Enhancements

- [ ] **Advanced Features**
- [ ] Multi-Raft support for sharding
- [ ] Read replicas for scaling
- [ ] Cross-datacenter replication
- [ ] Built-in backup/restore

- [ ] **Operations & Monitoring**
- [ ] Prometheus metrics export
- [ ] Grafana dashboards
- [ ] Alert manager integration
- [ ] Automatic failover scripts

- [ ] **Developer Experience**
- [ ] Client SDKs (Python, Go, Java, JavaScript)
- [ ] CLI management tool
- [ ] Web UI for cluster management
- [ ] Migration tools from other KV stores

## Contributing

We welcome contributions! Please see our contributing guidelines:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Add tests for new functionality
4. Ensure all tests pass (`cargo test`)
5. Update documentation as needed
6. Submit a pull request

### Development Setup

```bash
# Clone and setup
git clone https://github.com/your-org/ferrium
cd ferrium

# Install development dependencies
cargo install cargo-watch
cargo install cargo-nextest

# Run tests in watch mode
cargo watch -x "nextest run"
```

## Logo & Branding

The Ferrium logo is available in multiple formats for different use cases:

- **`docs/assets/logo.svg`** - Main logo for light backgrounds (README, documentation)
- **`docs/assets/logo-dark.svg`** - Optimized version for dark themes and backgrounds
- **`docs/assets/logo-icon.svg`** - Compact icon version for favicons and small displays

### Usage Guidelines

```html
<!-- For websites and documentation -->
<img src="docs/assets/logo.svg" alt="Ferrium" width="400"/>

<!-- For dark themes -->
<img src="docs/assets/logo-dark.svg" alt="Ferrium" width="400"/>

<!-- For favicons (convert to PNG/ICO as needed) -->
<img src="docs/assets/logo-icon.svg" alt="Ferrium" width="32"/>
```

The logo combines the iron/metal theme (Ferrium = iron) with distributed systems concepts, featuring connected nodes and modern gradients that represent the robust, interconnected nature of the system. The subtitle uses high-contrast colors to ensure readability against the metal accent elements.

## License

This project is licensed under the MIT OR Apache-2.0 license.

## Acknowledgments

- **[openraft](https://github.com/databendlabs/openraft)** - Robust Raft consensus implementation
- **[RocksDB](https://rocksdb.org/)** - High-performance storage engine
- **[tokio](https://tokio.rs/)** - Async runtime for Rust
- **[actix-web](https://actix.rs/)** - Fast HTTP framework
- **[tonic](https://github.com/hyperium/tonic)** - gRPC implementation
- Inspired by **etcd**, **Consul**, and **TiKV**

---

** For detailed configuration options, see [CONFIG.md](CONFIG.md)**

** Ready to build distributed systems? Start with `ferrium-server --generate-config ferrium.toml`**
