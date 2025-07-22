# Ferrite Configuration System

Ferrite supports comprehensive configuration through TOML files, with full CLI override support for flexible deployment scenarios.

## Quick Start

1. **Generate default configuration:**
   ```bash
   ferrite-server --generate-config ferrite.toml
   ```

2. **Validate configuration:**
   ```bash
   ferrite-server --config ferrite.toml --validate-config
   ```

3. **Run with configuration:**
   ```bash
   ferrite-server --config ferrite.toml
   ```

## Configuration File Locations

Ferrite searches for configuration files in the following order:

```bash
ferrite-server --list-config-paths
```

1. `ferrite.toml` (current directory)
2. `config/ferrite.toml` 
3. `/etc/ferrite/config.toml`
4. `~/.config/ferrite/config.toml`
5. `~/.ferrite.toml`

## Configuration Sections

### Node Configuration
```toml
[node]
id = 1                                    # Unique node identifier
http_addr = "127.0.0.1:8001"            # HTTP API bind address
grpc_addr = "127.0.0.1:9001"            # gRPC API bind address
data_dir = "./data"                      # Data directory
name = "ferrite-node-1"                 # Optional node name
description = "Primary node"             # Optional description
```

### Network Configuration
```toml
[network]
request_timeout = 30000                  # Request timeout (ms)
connect_timeout = 10000                  # Connection timeout (ms)
keep_alive_interval = 60000              # Keep-alive interval (ms)
max_retries = 3                          # Maximum retry attempts
retry_delay = 100                        # Retry delay base (ms)
enable_compression = true                # Enable network compression
max_message_size = 4194304               # Max message size (bytes)
```

### Storage Configuration
```toml
[storage]
enable_compression = true                # Enable data compression
compaction_strategy = "level"            # "level", "universal", "fifo"
max_log_size = 104857600                # Max log file size (bytes)
log_retention_count = 10                 # Number of log files to keep
enable_wal = true                        # Enable write-ahead logging
sync_writes = false                      # Sync writes to disk
block_cache_size = 64                    # Block cache size (MB)
write_buffer_size = 64                   # Write buffer size (MB)
```

### Raft Configuration
```toml
[raft]
heartbeat_interval = 250                 # Heartbeat interval (ms)
election_timeout_min = 299               # Min election timeout (ms)
election_timeout_max = 500               # Max election timeout (ms)
max_append_entries = 100                 # Max entries per request
enable_auto_compaction = true            # Enable auto log compaction
compaction_threshold = 1000              # Compaction threshold
max_inflight_requests = 10               # Max concurrent requests

[raft.snapshot_policy]
enable_auto_snapshot = true              # Enable automatic snapshots
entries_since_last_snapshot = 1000       # Entries before snapshot
snapshot_interval = 300000               # Snapshot interval (ms)
```

### Logging Configuration
```toml
[logging]
level = "info"                          # "trace", "debug", "info", "warn", "error"
format = "pretty"                       # "json", "pretty", "compact"
structured = false                      # Enable structured logging
file_path = "./logs/ferrite.log"        # Log file path (optional)
max_file_size = 104857600               # Max log file size (bytes)
max_files = 5                           # Number of log files to keep
enable_colors = true                    # Enable ANSI colors
```

### Cluster Configuration
```toml
[cluster]
name = "ferrite-cluster"                # Cluster name
expected_size = 3                       # Expected cluster size

# Define peer nodes
[cluster.peers.2]
http_addr = "10.0.1.11:8001"
grpc_addr = "10.0.1.11:9001"
priority = 90                           # Election priority
voting = true                           # Voting member

[cluster.discovery]
enabled = false                         # Enable peer discovery
method = "static"                       # "static", "dns", "consul", etc.
interval = 30000                        # Discovery interval (ms)
```

### Security Configuration
```toml
[security]
enable_tls = true                       # Enable TLS
cert_file = "/etc/ferrite/tls/server.crt"
key_file = "/etc/ferrite/tls/server.key"
ca_file = "/etc/ferrite/tls/ca.crt"
enable_mtls = true                      # Enable mutual TLS
auth_method = "certificate"             # "none", "token", "certificate", "jwt"
```

## CLI Overrides

Any configuration option can be overridden via command line:

```bash
# Override node configuration
ferrite-server --config ferrite.toml --id 42 --http-addr 0.0.0.0:8080

# Override logging
ferrite-server --config ferrite.toml --log-level debug

# Override data directory
ferrite-server --config ferrite.toml --data-dir /nvme/ferrite
```

## Example Configurations

### Single Node (Development)
```bash
ferrite-server --config examples/configs/single-node.toml
```

### Cluster Node (Production)
```bash
ferrite-server --config examples/configs/cluster-node1.toml
```

### High Performance
```bash
ferrite-server --config examples/configs/high-performance.toml
```

## Performance Tuning

### High Throughput
- Set `sync_writes = false` for performance
- Increase `write_buffer_size` and `block_cache_size`
- Use `compaction_strategy = "universal"`
- Disable compression for CPU efficiency
- Increase `max_append_entries` and `max_inflight_requests`

### High Durability
- Set `sync_writes = true` for data safety
- Enable `enable_wal = true`
- Use frequent snapshots
- Enable TLS and authentication

### Low Latency
- Reduce `heartbeat_interval` and election timeouts
- Decrease `request_timeout` and retry delays
- Use NVMe storage for `data_dir`
- Minimize logging level

## Configuration Validation

The configuration system performs comprehensive validation:

- **Node ID**: Must be non-zero and unique
- **Network**: Timeouts must be reasonable
- **Addresses**: HTTP and gRPC addresses must be different
- **Storage**: Directory paths must be accessible
- **Raft**: Election timeouts must be properly ordered
- **Logging**: Log level must be valid

## Environment Integration

### Systemd Service
```ini
[Unit]
Description=Ferrite Distributed KV Store
After=network.target

[Service]
Type=simple
User=ferrite
ExecStart=/usr/local/bin/ferrite-server --config /etc/ferrite/config.toml
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

### Docker
```dockerfile
FROM debian:bullseye-slim
COPY ferrite-server /usr/local/bin/
COPY config.toml /etc/ferrite/
EXPOSE 8001 9001
CMD ["ferrite-server", "--config", "/etc/ferrite/config.toml"]
```

### Kubernetes
```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: ferrite-config
data:
  config.toml: |
    [node]
    id = 1
    http_addr = "0.0.0.0:8001"
    # ... rest of config
```

## Best Practices

1. **Use configuration files** for complex deployments
2. **Version control** your configuration files
3. **Validate configurations** before deployment
4. **Use CLI overrides** for environment-specific values
5. **Monitor resource usage** and tune accordingly
6. **Test configurations** in staging environments
7. **Use structured logging** in production
8. **Enable TLS** for production deployments 