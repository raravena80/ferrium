use ferrium::config::{
    AuthMethod, ClusterConfig, FerriumConfig, LoggingConfig, NetworkConfig, NodeConfig,
    PeerConfigWithId, RaftConfig, SecurityConfig, SnapshotPolicy, StorageConfig,
};
use ferrium::tls::ClientTlsConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Integration test specifically for TLS auto-join functionality
/// This simulates the exact scenario that was failing before the fix
struct TlsAutoJoinTestSetup {
    _temp_dir: TempDir,
    ca_cert_path: PathBuf,
    node_certs: HashMap<u64, (PathBuf, PathBuf)>, // node_id -> (cert_path, key_path)
}

impl TlsAutoJoinTestSetup {
    pub fn new() -> std::io::Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path();

        // Generate CA certificate
        let ca_cert_path = temp_path.join("ca-cert.pem");
        let ca_key_path = temp_path.join("ca-key.pem");

        // Generate CA private key
        std::process::Command::new("openssl")
            .args(["genrsa", "-out", ca_key_path.to_str().unwrap(), "2048"])
            .output()?;

        // Generate CA certificate
        std::process::Command::new("openssl")
            .args([
                "req",
                "-new",
                "-x509",
                "-key",
                ca_key_path.to_str().unwrap(),
                "-sha256",
                "-subj",
                "/C=US/ST=CA/O=Ferrium Cluster Test/CN=Ferrium Test CA",
                "-days",
                "365",
                "-out",
                ca_cert_path.to_str().unwrap(),
            ])
            .output()?;

        let mut node_certs = HashMap::new();

        // Generate certificates for nodes 1, 2, 3
        for node_id in 1..=3 {
            let node_cert_path = temp_path.join(format!("node{node_id}-cert.pem"));
            let node_key_path = temp_path.join(format!("node{node_id}-key.pem"));

            // Generate node private key
            std::process::Command::new("openssl")
                .args(["genrsa", "-out", node_key_path.to_str().unwrap(), "2048"])
                .output()?;

            // Generate node certificate signing request
            let node_csr_path = temp_path.join(format!("node{node_id}.csr"));
            std::process::Command::new("openssl")
                .args([
                    "req",
                    "-new",
                    "-key",
                    node_key_path.to_str().unwrap(),
                    "-subj",
                    &format!("/C=US/ST=CA/O=Ferrium Cluster Test/CN=ferrium-node-{node_id}"),
                    "-out",
                    node_csr_path.to_str().unwrap(),
                ])
                .output()?;

            // Sign node certificate with CA
            std::process::Command::new("openssl")
                .args([
                    "x509",
                    "-req",
                    "-in",
                    node_csr_path.to_str().unwrap(),
                    "-CA",
                    ca_cert_path.to_str().unwrap(),
                    "-CAkey",
                    ca_key_path.to_str().unwrap(),
                    "-CAcreateserial",
                    "-out",
                    node_cert_path.to_str().unwrap(),
                    "-days",
                    "365",
                    "-sha256",
                ])
                .output()?;

            node_certs.insert(node_id, (node_cert_path, node_key_path));
        }

        Ok(TlsAutoJoinTestSetup {
            _temp_dir: temp_dir,
            ca_cert_path,
            node_certs,
        })
    }

    pub fn create_node_security_config(
        &self,
        node_id: u64,
        accept_invalid_certs: bool,
    ) -> SecurityConfig {
        let (cert_path, key_path) = self.node_certs.get(&node_id).unwrap();
        SecurityConfig {
            enable_tls: true,
            enable_mtls: false,
            cert_file: Some(cert_path.clone()),
            key_file: Some(key_path.clone()),
            ca_file: Some(self.ca_cert_path.clone()),
            accept_invalid_certs,
            auth_method: AuthMethod::None,
        }
    }

    pub fn create_test_config(
        &self,
        node_id: u64,
        http_port: u16,
        grpc_port: u16,
        accept_invalid_certs: bool,
        data_dir: PathBuf,
    ) -> FerriumConfig {
        let security_config = self.create_node_security_config(node_id, accept_invalid_certs);

        // Create peer configurations for the cluster
        let peers = vec![
            PeerConfigWithId {
                id: 1,
                http_addr: "127.0.0.1:21001".parse().unwrap(),
                grpc_addr: "127.0.0.1:31001".parse().unwrap(),
                priority: Some(100),
                voting: true,
            },
            PeerConfigWithId {
                id: 2,
                http_addr: "127.0.0.1:21002".parse().unwrap(),
                grpc_addr: "127.0.0.1:31002".parse().unwrap(),
                priority: Some(50),
                voting: true,
            },
            PeerConfigWithId {
                id: 3,
                http_addr: "127.0.0.1:21003".parse().unwrap(),
                grpc_addr: "127.0.0.1:31003".parse().unwrap(),
                priority: Some(50),
                voting: true,
            },
        ];

        FerriumConfig {
            node: NodeConfig {
                id: node_id,
                http_addr: format!("127.0.0.1:{http_port}").parse().unwrap(),
                grpc_addr: format!("127.0.0.1:{grpc_port}").parse().unwrap(),
                data_dir,
                name: Some(format!("test-node-{node_id}")),
                description: Some(format!("Test node {node_id} for TLS auto-join")),
            },
            network: NetworkConfig::default(),
            storage: StorageConfig::default(),
            raft: RaftConfig {
                heartbeat_interval: Duration::from_millis(100),
                election_timeout_min: Duration::from_millis(150),
                election_timeout_max: Duration::from_millis(300),
                max_append_entries: 50,
                enable_auto_compaction: true,
                compaction_threshold: 200,
                max_inflight_requests: 5,
                snapshot_policy: SnapshotPolicy::default(),
            },
            logging: LoggingConfig::default(),
            cluster: ClusterConfig {
                name: "tls-auto-join-test".to_string(),
                expected_size: Some(3),
                enable_auto_join: true,
                leader_discovery_timeout: Duration::from_secs(30),
                auto_join_timeout: Duration::from_secs(60),
                auto_join_retry_interval: Duration::from_secs(5),
                auto_accept_learners: true,
                peers: HashMap::new(), // Leave empty for now
                peer_list: peers,
                discovery: ferrium::config::DiscoveryConfig::default(),
            },
            security: security_config,
        }
    }
}

#[tokio::test]
async fn test_tls_auto_join_with_permissive_certificate_validation() {
    // Initialize crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    println!("🧪 Testing TLS auto-join with permissive certificate validation...");

    let setup = TlsAutoJoinTestSetup::new().expect("Failed to setup TLS certificates");

    // Create configurations for 3 nodes with permissive certificate validation
    let temp_dir1 = tempfile::tempdir().unwrap();
    let temp_dir2 = tempfile::tempdir().unwrap();
    let temp_dir3 = tempfile::tempdir().unwrap();

    let config1 = setup.create_test_config(1, 21001, 31001, true, temp_dir1.path().to_path_buf());
    let config2 = setup.create_test_config(2, 21002, 31002, true, temp_dir2.path().to_path_buf());
    let config3 = setup.create_test_config(3, 21003, 31003, true, temp_dir3.path().to_path_buf());

    // Test that we can create TLS configurations from these security configs
    let client_tls_config1 = ClientTlsConfig::from_security_config(&config1.security)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    let client_tls_config2 = ClientTlsConfig::from_security_config(&config2.security)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    let client_tls_config3 = ClientTlsConfig::from_security_config(&config3.security)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    // Verify all configs are in permissive mode
    assert!(
        client_tls_config1.accept_invalid_certs,
        "Node 1 should accept invalid certs"
    );
    assert!(
        client_tls_config2.accept_invalid_certs,
        "Node 2 should accept invalid certs"
    );
    assert!(
        client_tls_config3.accept_invalid_certs,
        "Node 3 should accept invalid certs"
    );

    assert!(
        !client_tls_config1.verify_hostname,
        "Node 1 should not verify hostname"
    );
    assert!(
        !client_tls_config2.verify_hostname,
        "Node 2 should not verify hostname"
    );
    assert!(
        !client_tls_config3.verify_hostname,
        "Node 3 should not verify hostname"
    );

    println!("✅ All node TLS configs created in permissive mode");

    // Test that we can create HTTP clients with these configurations
    // This simulates the network layer behavior during auto-join
    let client_tls_config = Arc::new(client_tls_config1);

    let client_builder = reqwest::Client::builder().timeout(Duration::from_secs(5));

    let client_builder = if client_tls_config.accept_invalid_certs {
        client_builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
    } else {
        match client_tls_config.create_rustls_client_config() {
            Ok(rustls_config) => {
                let config = Arc::try_unwrap(rustls_config).unwrap_or_else(|arc| (*arc).clone());
                client_builder.use_preconfigured_tls(config)
            }
            Err(e) => panic!("Failed to create TLS config: {e}"),
        }
    };

    let _client = client_builder.build().expect("Failed to build HTTP client");

    println!("✅ HTTP client with permissive TLS created successfully");
}

#[tokio::test]
async fn test_tls_auto_join_with_strict_certificate_validation() {
    // Initialize crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    println!("🧪 Testing TLS auto-join with strict certificate validation...");

    let setup = TlsAutoJoinTestSetup::new().expect("Failed to setup TLS certificates");

    // Create configurations for 3 nodes with strict certificate validation
    let temp_dir1 = tempfile::tempdir().unwrap();
    let temp_dir2 = tempfile::tempdir().unwrap();
    let temp_dir3 = tempfile::tempdir().unwrap();

    let config1 = setup.create_test_config(1, 21001, 31001, false, temp_dir1.path().to_path_buf());
    let config2 = setup.create_test_config(2, 21002, 31002, false, temp_dir2.path().to_path_buf());
    let config3 = setup.create_test_config(3, 21003, 31003, false, temp_dir3.path().to_path_buf());

    // Test that we can create TLS configurations from these security configs
    let client_tls_config1 = ClientTlsConfig::from_security_config(&config1.security)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    let client_tls_config2 = ClientTlsConfig::from_security_config(&config2.security)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    let client_tls_config3 = ClientTlsConfig::from_security_config(&config3.security)
        .expect("Failed to create client TLS config")
        .expect("Expected Some(ClientTlsConfig)");

    // Verify all configs are in strict mode
    assert!(
        !client_tls_config1.accept_invalid_certs,
        "Node 1 should NOT accept invalid certs"
    );
    assert!(
        !client_tls_config2.accept_invalid_certs,
        "Node 2 should NOT accept invalid certs"
    );
    assert!(
        !client_tls_config3.accept_invalid_certs,
        "Node 3 should NOT accept invalid certs"
    );

    assert!(
        client_tls_config1.verify_hostname,
        "Node 1 should verify hostname"
    );
    assert!(
        client_tls_config2.verify_hostname,
        "Node 2 should verify hostname"
    );
    assert!(
        client_tls_config3.verify_hostname,
        "Node 3 should verify hostname"
    );

    println!("✅ All node TLS configs created in strict mode");

    // Test that we can create proper TLS configurations for production use
    let client_tls_config = Arc::new(client_tls_config1);

    let client_builder = reqwest::Client::builder().timeout(Duration::from_secs(5));

    let client_builder = if client_tls_config.accept_invalid_certs {
        client_builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
    } else {
        // This should work with proper certificate validation
        match client_tls_config.create_rustls_client_config() {
            Ok(rustls_config) => {
                let config = Arc::try_unwrap(rustls_config).unwrap_or_else(|arc| (*arc).clone());
                client_builder.use_preconfigured_tls(config)
            }
            Err(e) => panic!("Failed to create TLS config: {e}"),
        }
    };

    let _client = client_builder.build().expect("Failed to build HTTP client");

    println!("✅ HTTP client with strict TLS created successfully");
}

#[tokio::test]
async fn test_network_layer_tls_configuration_switching() {
    // Initialize crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    println!("🧪 Testing network layer TLS configuration switching...");

    let setup = TlsAutoJoinTestSetup::new().expect("Failed to setup TLS certificates");

    // Test both permissive and strict modes
    for (mode_name, accept_invalid_certs) in [("permissive", true), ("strict", false)] {
        println!("  Testing {mode_name} mode...");

        let temp_dir = tempfile::tempdir().unwrap();
        let config = setup.create_test_config(
            1,
            21001,
            31001,
            accept_invalid_certs,
            temp_dir.path().to_path_buf(),
        );

        // Create client TLS config (this is what the network layer uses)
        let client_tls_config = ClientTlsConfig::from_security_config(&config.security)
            .expect("Failed to create client TLS config")
            .expect("Expected Some(ClientTlsConfig)");

        let client_tls_config = Arc::new(client_tls_config);

        // This simulates the exact logic in src/network/mod.rs
        let client_builder = reqwest::Client::builder().timeout(Duration::from_secs(5));

        let client_builder = if client_tls_config.accept_invalid_certs {
            // Permissive mode - for test environments with self-signed certificates
            client_builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
        } else {
            // Strict mode - for production environments with proper certificates
            match client_tls_config.create_rustls_client_config() {
                Ok(rustls_config) => {
                    let config =
                        Arc::try_unwrap(rustls_config).unwrap_or_else(|arc| (*arc).clone());
                    client_builder.use_preconfigured_tls(config)
                }
                Err(e) => panic!("Failed to create proper TLS config in strict mode: {e}"),
            }
        };

        let _client = client_builder
            .build()
            .unwrap_or_else(|_| panic!("Failed to build HTTP client in {mode_name} mode"));

        println!("  ✅ {mode_name} mode client created successfully");
    }

    println!("✅ Network layer TLS configuration switching test passed");
}

#[tokio::test]
async fn test_config_serialization_roundtrip() {
    // Initialize crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    println!("🧪 Testing configuration serialization/deserialization roundtrip...");

    let setup = TlsAutoJoinTestSetup::new().expect("Failed to setup TLS certificates");
    let temp_dir = tempfile::tempdir().unwrap();

    // Test both modes
    for (mode_name, accept_invalid_certs) in [("permissive", true), ("strict", false)] {
        println!("  Testing {mode_name} mode config roundtrip...");

        let original_config = setup.create_test_config(
            1,
            21001,
            31001,
            accept_invalid_certs,
            temp_dir.path().to_path_buf(),
        );

        // Serialize to TOML
        let toml_string =
            toml::to_string(&original_config).expect("Failed to serialize config to TOML");

        println!("  Generated TOML contains accept_invalid_certs = {accept_invalid_certs}");
        assert!(
            toml_string.contains(&format!("accept_invalid_certs = {accept_invalid_certs}")),
            "TOML should contain the correct accept_invalid_certs value"
        );

        // Deserialize from TOML
        let deserialized_config: FerriumConfig =
            toml::from_str(&toml_string).expect("Failed to deserialize config from TOML");

        // Verify the accept_invalid_certs field was preserved
        assert_eq!(
            deserialized_config.security.accept_invalid_certs, accept_invalid_certs,
            "accept_invalid_certs field should be preserved during roundtrip"
        );

        // Verify other security fields are preserved
        assert_eq!(
            deserialized_config.security.enable_tls,
            original_config.security.enable_tls
        );
        assert_eq!(
            deserialized_config.security.enable_mtls,
            original_config.security.enable_mtls
        );
        assert_eq!(
            deserialized_config.security.auth_method,
            original_config.security.auth_method
        );

        println!("  ✅ {mode_name} mode config roundtrip successful");
    }

    println!("✅ Configuration serialization roundtrip test passed");
}

#[tokio::test]
async fn test_cluster_peer_configuration_with_tls() {
    // Initialize crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    println!("🧪 Testing cluster peer configuration with TLS...");

    let setup = TlsAutoJoinTestSetup::new().expect("Failed to setup TLS certificates");

    let temp_dir1 = tempfile::tempdir().unwrap();
    let temp_dir2 = tempfile::tempdir().unwrap();
    let temp_dir3 = tempfile::tempdir().unwrap();

    // Create configurations for a 3-node TLS cluster with permissive certificate validation
    let config1 = setup.create_test_config(1, 21001, 31001, true, temp_dir1.path().to_path_buf());
    let config2 = setup.create_test_config(2, 21002, 31002, true, temp_dir2.path().to_path_buf());
    let config3 = setup.create_test_config(3, 21003, 31003, true, temp_dir3.path().to_path_buf());

    // Verify each node has the same peer configuration (required for auto-join)
    assert_eq!(
        config1.cluster.peer_list.len(),
        3,
        "Node 1 should know about 3 peers"
    );
    assert_eq!(
        config2.cluster.peer_list.len(),
        3,
        "Node 2 should know about 3 peers"
    );
    assert_eq!(
        config3.cluster.peer_list.len(),
        3,
        "Node 3 should know about 3 peers"
    );

    // Verify auto-join is enabled
    assert!(
        config1.cluster.enable_auto_join,
        "Node 1 should have auto-join enabled"
    );
    assert!(
        config2.cluster.enable_auto_join,
        "Node 2 should have auto-join enabled"
    );
    assert!(
        config3.cluster.enable_auto_join,
        "Node 3 should have auto-join enabled"
    );

    // Verify auto-accept is enabled (required for automatic cluster formation)
    assert!(
        config1.cluster.auto_accept_learners,
        "Node 1 should auto-accept learners"
    );
    assert!(
        config2.cluster.auto_accept_learners,
        "Node 2 should auto-accept learners"
    );
    assert!(
        config3.cluster.auto_accept_learners,
        "Node 3 should auto-accept learners"
    );

    // Verify TLS is enabled for all nodes
    assert!(
        config1.security.enable_tls,
        "Node 1 should have TLS enabled"
    );
    assert!(
        config2.security.enable_tls,
        "Node 2 should have TLS enabled"
    );
    assert!(
        config3.security.enable_tls,
        "Node 3 should have TLS enabled"
    );

    // Verify permissive certificate validation
    assert!(
        config1.security.accept_invalid_certs,
        "Node 1 should accept invalid certs"
    );
    assert!(
        config2.security.accept_invalid_certs,
        "Node 2 should accept invalid certs"
    );
    assert!(
        config3.security.accept_invalid_certs,
        "Node 3 should accept invalid certs"
    );

    println!("✅ Cluster peer configuration with TLS verified");
}
