use serde::{Deserialize, Serialize};
use serde_with::{DurationMilliSeconds, serde_as};
use std::collections::HashMap;
use std::io::Cursor;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// Node ID type - using u64 as mentioned in the user's requirements
pub type NodeId = u64;

/// Request type for the KV operations
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum KvRequest {
    Set { key: String, value: String },
    Get { key: String },
    Delete { key: String },
}

/// Response type for KV operations
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum KvResponse {
    Set,
    Get { value: Option<String> },
    Delete,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
pub struct Node {
    pub rpc_addr: String,
    pub api_addr: String,
}

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Node {{ rpc_addr: {}, api_addr: {} }}",
            self.rpc_addr, self.api_addr
        )
    }
}

pub type SnapshotData = Cursor<Vec<u8>>;

// Use the openraft macro to declare types instead of manual implementation
openraft::declare_raft_types!(
    pub TypeConfig:
        D = KvRequest,
        R = KvResponse,
        Node = Node,
);

/// Snapshot data structure for the KV store
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct KvSnapshot {
    pub data: std::collections::HashMap<String, String>,
}

/// Complete Ferrite configuration structure
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FerriteConfig {
    pub node: NodeConfig,
    pub network: NetworkConfig,
    pub storage: StorageConfig,
    pub raft: RaftConfig,
    pub logging: LoggingConfig,
    pub cluster: ClusterConfig,
    pub security: SecurityConfig,
}

/// Node-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Unique node identifier
    pub id: NodeId,
    /// HTTP API bind address
    pub http_addr: SocketAddr,
    /// gRPC API bind address  
    pub grpc_addr: SocketAddr,
    /// Data directory for persistent storage
    pub data_dir: PathBuf,
    /// Node name (optional, for display purposes)
    pub name: Option<String>,
    /// Node description (optional)
    pub description: Option<String>,
}

/// Network configuration
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Request timeout for client operations
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub request_timeout: Duration,
    /// Connection timeout for establishing connections
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub connect_timeout: Duration,
    /// Keep-alive interval for HTTP connections
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub keep_alive_interval: Duration,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Retry delay base (exponential backoff)
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub retry_delay: Duration,
    /// Enable compression for network communication
    pub enable_compression: bool,
    /// Maximum message size (in bytes)
    pub max_message_size: usize,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Enable data compression
    pub enable_compression: bool,
    /// Compaction strategy
    pub compaction_strategy: CompactionStrategy,
    /// Maximum log file size before rotation
    pub max_log_size: u64,
    /// Number of log files to retain
    pub log_retention_count: u32,
    /// Enable write-ahead logging
    pub enable_wal: bool,
    /// Sync writes to disk (durability vs performance)
    pub sync_writes: bool,
    /// Block cache size (in MB)
    pub block_cache_size: u32,
    /// Write buffer size (in MB)
    pub write_buffer_size: u32,
}

/// Compaction strategy for RocksDB
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStrategy {
    Level,
    Universal,
    Fifo,
}

/// Raft-specific configuration
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftConfig {
    /// Heartbeat interval
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub heartbeat_interval: Duration,
    /// Minimum election timeout
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub election_timeout_min: Duration,
    /// Maximum election timeout
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub election_timeout_max: Duration,
    /// Maximum entries per append entries request
    pub max_append_entries: u32,
    /// Snapshot policy
    pub snapshot_policy: SnapshotPolicy,
    /// Enable automatic log compaction
    pub enable_auto_compaction: bool,
    /// Log compaction threshold (number of entries)
    pub compaction_threshold: u64,
    /// Maximum number of inflight append entries requests
    pub max_inflight_requests: u32,
}

/// Snapshot creation policy
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPolicy {
    /// Enable automatic snapshots
    pub enable_auto_snapshot: bool,
    /// Number of log entries before creating snapshot
    pub entries_since_last_snapshot: u64,
    /// Time interval for automatic snapshots
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub snapshot_interval: Duration,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
    /// Log format (json, pretty, compact)
    pub format: LogFormat,
    /// Enable structured logging
    pub structured: bool,
    /// Log file path (None = stdout)
    pub file_path: Option<PathBuf>,
    /// Maximum log file size before rotation
    pub max_file_size: Option<u64>,
    /// Number of rotated log files to keep
    pub max_files: Option<u32>,
    /// Enable ANSI colors in output
    pub enable_colors: bool,
}

/// Log output format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

/// Cluster configuration
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Known peers for cluster discovery (HashMap format - optional)
    #[serde(default)]
    pub peers: HashMap<NodeId, PeerConfig>,
    /// Known peers for cluster discovery (Array format - preferred)
    #[serde(default, rename = "peer")]
    pub peer_list: Vec<PeerConfigWithId>,
    /// Auto-discovery settings
    pub discovery: DiscoveryConfig,
    /// Cluster name (for identification)
    pub name: String,
    /// Expected cluster size (for initialization)
    pub expected_size: Option<u32>,
    /// Enable automatic joining as learners
    pub enable_auto_join: bool,
    /// Timeout for leader discovery during auto-join
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub leader_discovery_timeout: Duration,
    /// Timeout for auto-join process
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub auto_join_timeout: Duration,
    /// Retry interval for auto-join attempts
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub auto_join_retry_interval: Duration,
    /// Auto-accept learner join requests (for leaders)
    pub auto_accept_learners: bool,
}

/// Individual peer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    /// Peer's HTTP address
    pub http_addr: SocketAddr,
    /// Peer's gRPC address
    pub grpc_addr: SocketAddr,
    /// Priority for leader election (higher = more likely to be leader)
    pub priority: Option<u32>,
    /// Whether this peer should participate in voting
    pub voting: bool,
}

/// Individual peer configuration with ID (for array format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfigWithId {
    /// Peer's node ID
    pub id: NodeId,
    /// Peer's HTTP address
    pub http_addr: SocketAddr,
    /// Peer's gRPC address
    pub grpc_addr: SocketAddr,
    /// Priority for leader election (higher = more likely to be leader)
    pub priority: Option<u32>,
    /// Whether this peer should participate in voting
    pub voting: bool,
}

/// Service discovery configuration
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Enable automatic peer discovery
    pub enabled: bool,
    /// Discovery method
    pub method: DiscoveryMethod,
    /// Discovery interval
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub interval: Duration,
}

/// Service discovery methods
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMethod {
    Static,
    Dns,
    Consul,
    Etcd,
    Kubernetes,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Enable TLS for all communication
    pub enable_tls: bool,
    /// Path to TLS certificate file
    pub cert_file: Option<PathBuf>,
    /// Path to TLS private key file
    pub key_file: Option<PathBuf>,
    /// Path to CA certificate for peer verification
    pub ca_file: Option<PathBuf>,
    /// Enable mutual TLS authentication
    pub enable_mtls: bool,
    /// Authentication method
    pub auth_method: AuthMethod,
}

/// Authentication methods
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    None,
    Token,
    Certificate,
    Jwt,
}

impl Default for FerriteConfig {
    fn default() -> Self {
        Self {
            node: NodeConfig::default(),
            network: NetworkConfig::default(),
            storage: StorageConfig::default(),
            raft: RaftConfig::default(),
            logging: LoggingConfig::default(),
            cluster: ClusterConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: 1,
            http_addr: "127.0.0.1:8001".parse().unwrap(),
            grpc_addr: "127.0.0.1:9001".parse().unwrap(),
            data_dir: PathBuf::from("./data"),
            name: None,
            description: None,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            keep_alive_interval: Duration::from_secs(60),
            max_retries: 3,
            retry_delay: Duration::from_millis(100),
            enable_compression: true,
            max_message_size: 4 * 1024 * 1024, // 4MB
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            enable_compression: true,
            compaction_strategy: CompactionStrategy::Level,
            max_log_size: 100 * 1024 * 1024, // 100MB
            log_retention_count: 10,
            enable_wal: true,
            sync_writes: false,
            block_cache_size: 64,  // 64MB
            write_buffer_size: 64, // 64MB
        }
    }
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(250),
            election_timeout_min: Duration::from_millis(299),
            election_timeout_max: Duration::from_millis(500),
            max_append_entries: 100,
            snapshot_policy: SnapshotPolicy::default(),
            enable_auto_compaction: true,
            compaction_threshold: 1000,
            max_inflight_requests: 10,
        }
    }
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self {
            enable_auto_snapshot: true,
            entries_since_last_snapshot: 1000,
            snapshot_interval: Duration::from_secs(300), // 5 minutes
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Pretty,
            structured: false,
            file_path: None,
            max_file_size: Some(100 * 1024 * 1024), // 100MB
            max_files: Some(5),
            enable_colors: true,
        }
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            peers: HashMap::new(),
            peer_list: Vec::new(),
            discovery: DiscoveryConfig::default(),
            name: "ferrite-cluster".to_string(),
            expected_size: None,
            enable_auto_join: true,
            leader_discovery_timeout: Duration::from_secs(30),
            auto_join_timeout: Duration::from_secs(60),
            auto_join_retry_interval: Duration::from_secs(5),
            auto_accept_learners: true,
        }
    }
}

impl ClusterConfig {
    /// Get all peers as a HashMap, combining both formats
    pub fn get_all_peers(&self) -> HashMap<NodeId, PeerConfig> {
        let mut all_peers = self.peers.clone();

        // Add peers from array format
        for peer_with_id in &self.peer_list {
            all_peers.insert(
                peer_with_id.id,
                PeerConfig {
                    http_addr: peer_with_id.http_addr,
                    grpc_addr: peer_with_id.grpc_addr,
                    priority: peer_with_id.priority,
                    voting: peer_with_id.voting,
                },
            );
        }

        all_peers
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            method: DiscoveryMethod::Static,
            interval: Duration::from_secs(30),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enable_tls: false,
            cert_file: None,
            key_file: None,
            ca_file: None,
            enable_mtls: false,
            auth_method: AuthMethod::None,
        }
    }
}

/// Configuration loading and management
impl FerriteConfig {
    /// Load configuration from file
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e))?;

        let config: Self =
            toml::from_str(&content).map_err(|e| ConfigError::Parse(e.to_string()))?;

        config.validate()?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn to_file<P: AsRef<std::path::Path>>(&self, path: P) -> Result<(), ConfigError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;

        std::fs::write(path, content).map_err(|e| ConfigError::Io(e))?;

        Ok(())
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate node ID
        if self.node.id == 0 {
            return Err(ConfigError::Validation("Node ID cannot be 0".to_string()));
        }

        // Validate timeouts
        if self.raft.election_timeout_min >= self.raft.election_timeout_max {
            return Err(ConfigError::Validation(
                "Election timeout min must be less than max".to_string(),
            ));
        }

        // Validate addresses are not conflicting
        if self.node.http_addr == self.node.grpc_addr {
            return Err(ConfigError::Validation(
                "HTTP and gRPC addresses cannot be the same".to_string(),
            ));
        }

        // Validate log level
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.logging.level.as_str()) {
            return Err(ConfigError::Validation(format!(
                "Invalid log level: {}. Must be one of: {:?}",
                self.logging.level, valid_levels
            )));
        }

        Ok(())
    }

    /// Get default configuration file paths
    pub fn default_config_paths() -> Vec<PathBuf> {
        let mut paths = vec![
            PathBuf::from("ferrite.toml"),
            PathBuf::from("config/ferrite.toml"),
            PathBuf::from("/etc/ferrite/config.toml"),
        ];

        if let Some(config_dir) = dirs::config_dir() {
            paths.push(config_dir.join("ferrite").join("config.toml"));
        }

        if let Some(home_dir) = dirs::home_dir() {
            paths.push(home_dir.join(".config").join("ferrite").join("config.toml"));
            paths.push(home_dir.join(".ferrite.toml"));
        }

        paths
    }

    /// Find and load configuration file from default locations
    pub fn load_default() -> Result<Self, ConfigError> {
        for path in Self::default_config_paths() {
            if path.exists() {
                return Self::from_file(&path);
            }
        }

        // No config file found, return default configuration
        Ok(Self::default())
    }
}

/// Configuration error types
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Serialization error: {0}")]
    Serialize(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

/// Create a default Raft configuration from the config struct
pub fn create_raft_config(config: &RaftConfig) -> openraft::Config {
    openraft::Config {
        heartbeat_interval: config.heartbeat_interval.as_millis() as u64,
        election_timeout_min: config.election_timeout_min.as_millis() as u64,
        election_timeout_max: config.election_timeout_max.as_millis() as u64,
        max_in_snapshot_log_to_keep: config.compaction_threshold,
        ..Default::default()
    }
}

/// Legacy function for backward compatibility
pub fn create_raft_config_default() -> openraft::Config {
    let default_raft_config = RaftConfig::default();
    create_raft_config(&default_raft_config)
}

// Configuration tests are in test.rs
#[cfg(test)]
mod test;
