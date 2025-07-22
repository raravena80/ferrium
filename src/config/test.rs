use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_default_config_creation() {
    let config = FerriteConfig::default();

    // Test default values
    assert_eq!(config.node.id, 1);
    assert_eq!(
        config.node.http_addr,
        "127.0.0.1:8001".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        config.node.grpc_addr,
        "127.0.0.1:9001".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.node.data_dir, PathBuf::from("./data"));
    assert!(config.node.name.is_none());

    // Test network defaults
    assert_eq!(config.network.request_timeout, Duration::from_secs(30));
    assert_eq!(config.network.max_retries, 3);
    assert!(config.network.enable_compression);

    // Test storage defaults
    assert!(config.storage.enable_compression);
    assert_eq!(
        config.storage.compaction_strategy,
        CompactionStrategy::Level
    );
    assert!(config.storage.enable_wal);

    // Test raft defaults
    assert_eq!(config.raft.heartbeat_interval, Duration::from_millis(250));
    assert_eq!(config.raft.election_timeout_min, Duration::from_millis(299));
    assert_eq!(config.raft.election_timeout_max, Duration::from_millis(500));

    // Test cluster defaults
    assert_eq!(config.cluster.name, "ferrite-cluster");
    assert!(config.cluster.enable_auto_join);
    assert!(config.cluster.auto_accept_learners);

    // Test security defaults
    assert!(!config.security.enable_tls);
    assert!(!config.security.enable_mtls);
    assert_eq!(config.security.auth_method, AuthMethod::None);
}

#[test]
fn test_config_validation_valid() {
    let config = FerriteConfig::default();
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_invalid_node_id() {
    let mut config = FerriteConfig::default();
    config.node.id = 0; // Invalid node ID

    let result = config.validate();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Node ID cannot be 0")
    );
}

#[test]
fn test_config_validation_invalid_timeouts() {
    let mut config = FerriteConfig::default();
    config.raft.election_timeout_min = Duration::from_millis(300);
    config.raft.election_timeout_max = Duration::from_millis(150); // Invalid: min > max

    let result = config.validate();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Election timeout min must be less than max")
    );
}

#[test]
fn test_config_validation_same_addresses() {
    let mut config = FerriteConfig::default();
    config.node.grpc_addr = config.node.http_addr; // Same addresses - invalid

    let result = config.validate();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("HTTP and gRPC addresses cannot be the same")
    );
}

#[test]
fn test_config_validation_invalid_log_level() {
    let mut config = FerriteConfig::default();
    config.logging.level = "invalid_level".to_string();

    let result = config.validate();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid log level")
    );
}

#[test]
fn test_toml_serialization_deserialization() {
    let config = FerriteConfig::default();

    // Serialize to TOML
    let toml_content = toml::to_string_pretty(&config).expect("Failed to serialize to TOML");
    assert!(!toml_content.is_empty());

    // Deserialize back
    let deserialized: FerriteConfig =
        toml::from_str(&toml_content).expect("Failed to deserialize from TOML");

    // Compare key fields (can't use PartialEq due to SocketAddr)
    assert_eq!(config.node.id, deserialized.node.id);
    assert_eq!(config.node.data_dir, deserialized.node.data_dir);
    assert_eq!(config.cluster.name, deserialized.cluster.name);
    assert_eq!(config.logging.level, deserialized.logging.level);
}

#[test]
fn test_toml_parsing_peer_array_format() {
    let toml_content = r#"
[node]
id = 1
http_addr = "127.0.0.1:8001"
grpc_addr = "127.0.0.1:9001"
data_dir = "./data"

[network]
request_timeout = 30000
connect_timeout = 10000
keep_alive_interval = 60000
max_retries = 3
retry_delay = 100
enable_compression = true
max_message_size = 4194304

[storage]
enable_compression = true
compaction_strategy = "level"
max_log_size = 104857600
log_retention_count = 10
enable_wal = true
sync_writes = false
block_cache_size = 64
write_buffer_size = 64

[raft]
heartbeat_interval = 250
election_timeout_min = 299
election_timeout_max = 500
max_append_entries = 100
enable_auto_compaction = true
compaction_threshold = 1000
max_inflight_requests = 10

[raft.snapshot_policy]
enable_auto_snapshot = true
entries_since_last_snapshot = 1000
snapshot_interval = 300000

[logging]
level = "info"
format = "pretty"
structured = false
enable_colors = true

[cluster]
name = "test-cluster"
expected_size = 3
enable_auto_join = true
leader_discovery_timeout = 15000
auto_join_timeout = 30000
auto_join_retry_interval = 2000
auto_accept_learners = true

[cluster.discovery]
enabled = false
method = "static"
interval = 30000

[[cluster.peer]]
id = 1
http_addr = "127.0.0.1:8001"
grpc_addr = "127.0.0.1:9001"
voting = true

[[cluster.peer]]
id = 2
http_addr = "127.0.0.1:8002"
grpc_addr = "127.0.0.1:9002"
voting = true

[security]
enable_tls = false
enable_mtls = false
auth_method = "none"
"#;

    let config: FerriteConfig =
        toml::from_str(toml_content).expect("Failed to parse TOML with peer array");

    assert_eq!(config.cluster.name, "test-cluster");
    assert!(config.cluster.enable_auto_join);
    assert!(config.cluster.auto_accept_learners);

    // Test peer array parsing
    assert_eq!(config.cluster.peer_list.len(), 2);
    assert_eq!(config.cluster.peer_list[0].id, 1);
    assert_eq!(
        config.cluster.peer_list[0].http_addr,
        "127.0.0.1:8001".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.cluster.peer_list[1].id, 2);
    assert_eq!(
        config.cluster.peer_list[1].http_addr,
        "127.0.0.1:8002".parse::<SocketAddr>().unwrap()
    );

    // Test get_all_peers method
    let all_peers = config.cluster.get_all_peers();
    assert_eq!(all_peers.len(), 2);
    assert!(all_peers.contains_key(&1));
    assert!(all_peers.contains_key(&2));
}

#[test]
fn test_file_io_operations() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("test_config.toml");

    let original_config = FerriteConfig::default();

    // Test saving to file
    original_config
        .to_file(&config_path)
        .expect("Failed to save config to file");
    assert!(config_path.exists());

    // Test loading from file
    let loaded_config =
        FerriteConfig::from_file(&config_path).expect("Failed to load config from file");

    // Compare key fields
    assert_eq!(original_config.node.id, loaded_config.node.id);
    assert_eq!(original_config.cluster.name, loaded_config.cluster.name);
    assert_eq!(original_config.logging.level, loaded_config.logging.level);
}

#[test]
fn test_file_io_invalid_toml() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("invalid_config.toml");

    // Write invalid TOML
    fs::write(&config_path, "invalid toml content [[[").expect("Failed to write file");

    let result = FerriteConfig::from_file(&config_path);
    assert!(result.is_err());

    match result.unwrap_err() {
        ConfigError::Parse(msg) => assert!(!msg.is_empty()),
        _ => panic!("Expected parse error"),
    }
}

#[test]
fn test_file_io_nonexistent_file() {
    let result = FerriteConfig::from_file("/nonexistent/path/config.toml");
    assert!(result.is_err());

    match result.unwrap_err() {
        ConfigError::Io(_) => (), // Expected IO error
        _ => panic!("Expected IO error"),
    }
}

#[test]
fn test_default_config_paths() {
    let paths = FerriteConfig::default_config_paths();

    // Should contain at least user config and system config paths
    assert!(paths.len() >= 2);

    // Check that paths contain expected patterns
    let paths_str: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    let combined = paths_str.join(" ");

    // Should contain user directory path
    assert!(combined.contains("ferrite") || combined.contains(".ferrite"));

    // Should contain system path
    assert!(combined.contains("/etc") || combined.contains("system"));
}

#[test]
fn test_load_default_config() {
    // This should not fail even if no config files exist
    // It should return the default configuration
    let result = FerriteConfig::load_default();
    assert!(result.is_ok());

    let config = result.unwrap();
    assert_eq!(config.node.id, 1); // Default node ID
    assert_eq!(config.cluster.name, "ferrite-cluster"); // Default cluster name
}

#[test]
fn test_config_error_display() {
    let io_error = ConfigError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "File not found",
    ));
    assert!(io_error.to_string().contains("File not found"));

    let parse_error = ConfigError::Parse("Invalid TOML".to_string());
    assert!(parse_error.to_string().contains("Invalid TOML"));

    let validation_error = ConfigError::Validation("Node ID cannot be 0".to_string());
    assert!(validation_error.to_string().contains("Node ID cannot be 0"));
}

#[test]
fn test_peer_config_with_id() {
    let peer = PeerConfigWithId {
        id: 42,
        http_addr: "192.168.1.100:8080".parse::<SocketAddr>().unwrap(),
        grpc_addr: "192.168.1.100:9090".parse::<SocketAddr>().unwrap(),
        priority: Some(10),
        voting: false,
    };

    assert_eq!(peer.id, 42);
    assert_eq!(
        peer.http_addr,
        "192.168.1.100:8080".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        peer.grpc_addr,
        "192.168.1.100:9090".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(peer.priority, Some(10));
    assert!(!peer.voting);
}

#[test]
fn test_get_all_peers_combined() {
    let mut config = FerriteConfig::default();

    // Add a peer using the old HashMap format
    let old_peer = PeerConfig {
        http_addr: "127.0.0.1:8001".parse::<SocketAddr>().unwrap(),
        grpc_addr: "127.0.0.1:9001".parse::<SocketAddr>().unwrap(),
        priority: None,
        voting: true,
    };
    config.cluster.peers.insert(1, old_peer);

    // Add a peer using the new array format
    let new_peer = PeerConfigWithId {
        id: 2,
        http_addr: "127.0.0.1:8002".parse::<SocketAddr>().unwrap(),
        grpc_addr: "127.0.0.1:9002".parse::<SocketAddr>().unwrap(),
        priority: None,
        voting: true,
    };
    config.cluster.peer_list.push(new_peer);

    let all_peers = config.cluster.get_all_peers();
    assert_eq!(all_peers.len(), 2);
    assert!(all_peers.contains_key(&1));
    assert!(all_peers.contains_key(&2));

    // Array format should override HashMap format for same ID
    let duplicate_peer = PeerConfigWithId {
        id: 1,                                                      // Same ID as HashMap peer
        http_addr: "127.0.0.1:8003".parse::<SocketAddr>().unwrap(), // Different address
        grpc_addr: "127.0.0.1:9003".parse::<SocketAddr>().unwrap(),
        priority: None,
        voting: false,
    };
    config.cluster.peer_list.push(duplicate_peer);

    let all_peers_with_override = config.cluster.get_all_peers();
    assert_eq!(all_peers_with_override.len(), 2); // Still 2 peers

    // The array format should have overridden the HashMap format
    let peer_1 = &all_peers_with_override[&1];
    assert_eq!(
        peer_1.http_addr,
        "127.0.0.1:8003".parse::<SocketAddr>().unwrap()
    ); // From array format
    assert!(!peer_1.voting); // From array format
}

#[test]
fn test_raft_config_creation() {
    let raft_config = RaftConfig::default();
    let openraft_config = create_raft_config(&raft_config);

    assert_eq!(
        openraft_config.heartbeat_interval,
        raft_config.heartbeat_interval.as_millis() as u64
    );
    assert_eq!(
        openraft_config.election_timeout_min,
        raft_config.election_timeout_min.as_millis() as u64
    );
    assert_eq!(
        openraft_config.election_timeout_max,
        raft_config.election_timeout_max.as_millis() as u64
    );
    assert_eq!(
        openraft_config.max_in_snapshot_log_to_keep,
        raft_config.compaction_threshold
    );
}

#[test]
fn test_raft_config_legacy_function() {
    let config1 = create_raft_config_default();
    let config2 = create_raft_config(&RaftConfig::default());

    assert_eq!(config1.heartbeat_interval, config2.heartbeat_interval);
    assert_eq!(config1.election_timeout_min, config2.election_timeout_min);
    assert_eq!(config1.election_timeout_max, config2.election_timeout_max);
}

#[test]
fn test_compaction_strategy_serialization() {
    // Test all compaction strategy variants
    let strategies = vec![
        CompactionStrategy::Level,
        CompactionStrategy::Universal,
        CompactionStrategy::Fifo,
    ];

    for strategy in strategies {
        let serialized = serde_json::to_string(&strategy).expect("Failed to serialize strategy");
        let deserialized: CompactionStrategy =
            serde_json::from_str(&serialized).expect("Failed to deserialize strategy");
        assert_eq!(strategy, deserialized);
    }
}

#[test]
fn test_auth_method_serialization() {
    // Test all auth method variants
    let methods = vec![
        AuthMethod::None,
        AuthMethod::Token,
        AuthMethod::Certificate,
        AuthMethod::Jwt,
    ];

    for method in methods {
        let serialized = serde_json::to_string(&method).expect("Failed to serialize auth method");
        let deserialized: AuthMethod =
            serde_json::from_str(&serialized).expect("Failed to deserialize auth method");
        assert_eq!(method, deserialized);
    }
}

#[test]
fn test_duration_serialization_with_serde_with() {
    // Test that durations are properly serialized as milliseconds
    let config = FerriteConfig::default();
    let toml_content = toml::to_string(&config).expect("Failed to serialize config");

    // Should contain duration values as milliseconds
    assert!(toml_content.contains("heartbeat_interval = 250"));
    assert!(toml_content.contains("election_timeout_min = 299"));
    assert!(toml_content.contains("request_timeout = 30000"));
}
