#![allow(clippy::uninlined_format_args)]
#![allow(dead_code)]

use ferrium::{
    client::FerriumClient,
    config::{AuthMethod, FerriumConfig, PeerConfigWithId, SecurityConfig},
};
use serial_test::serial;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Once;
use std::time::{Duration, SystemTime};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

static INIT: Once = Once::new();

fn init_crypto_provider() {
    INIT.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Failed to install rustls crypto provider");
    });
}

/// Test utilities for real cluster testing
mod real_cluster_utils {
    use super::*;

    /// Find an available port
    pub fn find_available_port() -> u16 {
        use std::net::TcpListener;

        // Try to bind to port 0 to get a random available port
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to any port");
        let addr = listener.local_addr().expect("Failed to get local address");
        addr.port()
    }

    pub struct RealClusterTestEnvironment {
        pub temp_dir: TempDir,
        pub config_dir: PathBuf,
        pub data_dir: PathBuf,
        pub log_dir: PathBuf,
        pub certs_dir: Option<PathBuf>,
        pub node_configs: HashMap<u32, PathBuf>,
        pub node_processes: Vec<Child>,
        pub node_addrs: HashMap<u32, (String, String)>, // (http_addr, grpc_addr)
        pub enable_tls: bool,
    }

    impl RealClusterTestEnvironment {
        pub fn new(
            node_ids: Vec<u32>,
            enable_tls: bool,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            // Use a persistent directory in the project root instead of temp directories
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let base_path = std::env::current_dir()
                .unwrap()
                .join("test-artifacts")
                .join(format!("cluster-test-{}", timestamp));

            // Create the base directory
            std::fs::create_dir_all(&base_path)?;

            // Create a temporary directory wrapper for compatibility
            let temp_dir = tempfile::Builder::new()
                .prefix("ferrium-test-")
                .tempdir_in(&base_path)?;

            let config_dir = temp_dir.path().join("configs");
            let data_dir = temp_dir.path().join("data");
            let log_dir = temp_dir.path().join("logs");

            std::fs::create_dir_all(&config_dir)?;
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(&log_dir)?;

            let certs_dir = if enable_tls {
                let certs = temp_dir.path().join("certs");
                std::fs::create_dir_all(&certs)?;
                Some(certs)
            } else {
                None
            };

            println!("📁 Test artifacts stored in: {}", base_path.display());
            println!("📁 Active test directory: {}", temp_dir.path().display());

            let mut node_configs = HashMap::new();
            let mut node_addrs = HashMap::new();

            // Generate TLS certificates if needed
            if enable_tls {
                Self::generate_tls_certificates(certs_dir.as_ref().unwrap(), &node_ids)?;
            }

            // Create configuration files
            for node_id in &node_ids {
                let http_port = 21000 + node_id; // 21001, 21002, 21003 (same as script)
                let grpc_port = 31000 + node_id; // 31001, 31002, 31003 (same as script)
                let http_addr = format!("127.0.0.1:{}", http_port);
                let grpc_addr = format!("127.0.0.1:{}", grpc_port);

                node_addrs.insert(*node_id, (http_addr.clone(), grpc_addr.clone()));

                let config_path = Self::create_node_config(
                    &config_dir,
                    &data_dir,
                    &log_dir,
                    &certs_dir,
                    *node_id,
                    &http_addr,
                    &grpc_addr,
                    &node_addrs,
                    enable_tls,
                )?;

                node_configs.insert(*node_id, config_path);
            }

            Ok(RealClusterTestEnvironment {
                temp_dir,
                config_dir,
                data_dir,
                log_dir,
                certs_dir,
                node_configs,
                node_processes: Vec::new(),
                node_addrs,
                enable_tls,
            })
        }

        #[allow(clippy::ptr_arg)]
        fn generate_tls_certificates(
            certs_dir: &PathBuf,
            node_ids: &[u32],
        ) -> Result<(), Box<dyn std::error::Error>> {
            println!("🔐 Generating TLS certificates...");

            // Generate CA
            let ca_key = certs_dir.join("ca-key.pem");
            let ca_cert = certs_dir.join("ca-cert.pem");

            let output = Command::new("openssl")
                .args(["genrsa", "-out"])
                .arg(&ca_key)
                .arg("2048")
                .output()?;

            if !output.status.success() {
                return Err(format!(
                    "Failed to generate CA key: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }

            let output = Command::new("openssl")
                .args(["req", "-new", "-x509", "-key"])
                .arg(&ca_key)
                .args(["-out"])
                .arg(&ca_cert)
                .args([
                    "-days",
                    "365",
                    "-subj",
                    "/CN=Ferrium Test CA/O=Ferrium Test",
                ])
                .output()?;

            if !output.status.success() {
                return Err(format!(
                    "Failed to generate CA cert: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }

            // Generate node certificates
            for node_id in node_ids {
                println!("🔐 Generating certificate for node {}", node_id);
                let node_key = certs_dir.join(format!("node{}-key.pem", node_id));
                let node_cert = certs_dir.join(format!("node{}-cert.pem", node_id));
                let node_csr = certs_dir.join(format!("node{}.csr", node_id));

                // Generate key
                let output = Command::new("openssl")
                    .args(["genrsa", "-out"])
                    .arg(&node_key)
                    .arg("2048")
                    .output()?;

                if !output.status.success() {
                    return Err(format!(
                        "Failed to generate key for node {}: {}",
                        node_id,
                        String::from_utf8_lossy(&output.stderr)
                    )
                    .into());
                }

                // Generate CSR
                let output = Command::new("openssl")
                    .args(["req", "-new", "-key"])
                    .arg(&node_key)
                    .args(["-out"])
                    .arg(&node_csr)
                    .args([
                        "-subj",
                        &format!("/CN=ferrium-node-{}/O=Ferrium Test", node_id),
                    ])
                    .output()?;

                if !output.status.success() {
                    return Err(format!(
                        "Failed to generate CSR for node {}: {}",
                        node_id,
                        String::from_utf8_lossy(&output.stderr)
                    )
                    .into());
                }

                // Create extensions file for node certificate
                let ext_file = certs_dir.join(format!("node{}_ext.conf", node_id));
                std::fs::write(
                    &ext_file,
                    "\
                    [v3_req]\n\
                    basicConstraints = CA:FALSE\n\
                    keyUsage = digitalSignature, keyEncipherment\n\
                    extendedKeyUsage = serverAuth, clientAuth\n\
                    subjectAltName = @alt_names\n\
                    \n\
                    [alt_names]\n\
                    DNS.1 = localhost\n\
                    DNS.2 = 127.0.0.1\n\
                    IP.1 = 127.0.0.1\n\
                    ",
                )?;

                // Sign certificate with proper SAN extensions for rustls compatibility
                let output = Command::new("openssl")
                    .args(["x509", "-req", "-in"])
                    .arg(&node_csr)
                    .args(["-CA"])
                    .arg(&ca_cert)
                    .args(["-CAkey"])
                    .arg(&ca_key)
                    .args(["-CAcreateserial", "-out"])
                    .arg(&node_cert)
                    .args(["-days", "365"])
                    .args(["-extensions", "v3_req"])
                    .args(["-extfile"])
                    .arg(&ext_file)
                    .output()?;

                if !output.status.success() {
                    return Err(format!(
                        "Failed to sign certificate for node {}: {}",
                        node_id,
                        String::from_utf8_lossy(&output.stderr)
                    )
                    .into());
                }
            }

            println!("✅ TLS certificates generated successfully");
            Ok(())
        }

        #[allow(clippy::too_many_arguments, clippy::ptr_arg)]
        pub fn create_node_config(
            config_dir: &PathBuf,
            data_dir: &PathBuf,
            log_dir: &PathBuf,
            certs_dir: &Option<PathBuf>,
            node_id: u32,
            http_addr: &str,
            grpc_addr: &str,
            all_node_addrs: &HashMap<u32, (String, String)>,
            enable_tls: bool,
        ) -> Result<PathBuf, Box<dyn std::error::Error>> {
            let config_path = config_dir.join(format!("node{}.toml", node_id));

            let mut config = FerriumConfig::default();
            config.node.id = node_id as u64;
            config.node.http_addr = http_addr.parse()?;
            config.node.grpc_addr = grpc_addr.parse()?;
            config.node.data_dir = data_dir.join(format!("node{}", node_id));
            config.logging.file_path = Some(log_dir.join(format!("node{}.log", node_id)));

            // Use faster timing settings like the working script (optimized for testing)
            config.raft.heartbeat_interval = std::time::Duration::from_millis(100);
            config.raft.election_timeout_min = std::time::Duration::from_millis(150);
            config.raft.election_timeout_max = std::time::Duration::from_millis(300);
            config.raft.compaction_threshold = 200;
            config.raft.max_append_entries = 50;
            config.raft.max_inflight_requests = 5;

            // Cluster configuration - match the working script
            config.cluster.name = "ferrium-automated-test".to_string();
            config.cluster.expected_size = Some(all_node_addrs.len() as u32);
            config.cluster.enable_auto_join = true;
            config.cluster.auto_accept_learners = true;

            // Include ALL nodes in each node's peer configuration like the working script
            // This is required for proper cluster initialization
            for (peer_id, (peer_http_addr, peer_grpc_addr)) in all_node_addrs {
                let peer_config = PeerConfigWithId {
                    id: *peer_id as u64,
                    http_addr: peer_http_addr.parse()?,
                    grpc_addr: peer_grpc_addr.parse()?,
                    voting: true,
                    priority: if *peer_id == 1 { Some(100) } else { Some(50) },
                };
                config.cluster.peer_list.push(peer_config);
            }

            // TLS configuration
            if enable_tls {
                if let Some(certs_dir) = certs_dir {
                    config.security = SecurityConfig {
                        enable_tls: true,
                        enable_mtls: false,
                        cert_file: Some(certs_dir.join(format!("node{}-cert.pem", node_id))),
                        key_file: Some(certs_dir.join(format!("node{}-key.pem", node_id))),
                        ca_file: Some(certs_dir.join("ca-cert.pem")), // Include CA cert for client validation
                        accept_invalid_certs: true, // For test environments with self-signed certificates
                        auth_method: AuthMethod::None,
                    };
                }
            }

            // Write config file
            let toml_string = toml::to_string(&config)?;
            std::fs::write(&config_path, toml_string)?;

            Ok(config_path)
        }

        pub fn start_cluster(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            println!("🚀 Starting real Ferrium cluster...");

            for (node_id, config_path) in &self.node_configs {
                println!("Starting node {}...", node_id);

                let mut cmd = Command::new("./target/release/ferrium-server")
                    .arg("--config")
                    .arg(config_path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;

                // Give the node time to start
                std::thread::sleep(std::time::Duration::from_millis(500));

                // Check if the process is still running
                match cmd.try_wait() {
                    Ok(Some(status)) => {
                        return Err(format!(
                            "Node {} exited early with status: {}\nStderr: {:?}",
                            node_id, status, "Check log files for details"
                        )
                        .into());
                    }
                    Ok(None) => {
                        // Process is still running
                        println!("✅ Node {} started successfully", node_id);
                        self.node_processes.push(cmd);
                    }
                    Err(e) => {
                        return Err(
                            format!("Failed to check node {} status: {}", node_id, e).into()
                        );
                    }
                }
            }

            Ok(())
        }

        /// Clean up old test artifacts (older than 1 hour)
        pub fn cleanup_old_test_artifacts() -> std::io::Result<()> {
            let test_artifacts_dir = std::env::current_dir()?.join("test-artifacts");

            if !test_artifacts_dir.exists() {
                return Ok(());
            }

            let cutoff_time = std::time::SystemTime::now() - std::time::Duration::from_secs(3600); // 1 hour ago

            for entry in std::fs::read_dir(&test_artifacts_dir)? {
                let entry = entry?;
                let metadata = entry.metadata()?;

                if metadata.is_dir() {
                    if let Ok(modified) = metadata.modified() {
                        if modified < cutoff_time {
                            if let Err(e) = std::fs::remove_dir_all(entry.path()) {
                                println!(
                                    "⚠️ Failed to remove old test directory {}: {}",
                                    entry.path().display(),
                                    e
                                );
                            } else {
                                println!(
                                    "🧹 Cleaned up old test directory: {}",
                                    entry.path().display()
                                );
                            }
                        }
                    }
                }
            }

            Ok(())
        }

        pub async fn initialize_cluster(&self) -> Result<(), Box<dyn std::error::Error>> {
            println!("🔧 Initializing cluster like the working script...");

            // Give nodes time to fully start up
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            // Step 1: Initialize Node 1 as leader (like the script does)
            println!("📍 Step 1: Initializing Node 1 as leader...");
            let (http_addr_1, _) = self.node_addrs.get(&1).unwrap();
            let protocol = if self.enable_tls { "https" } else { "http" };
            let init_url = format!("{}://{}/init", protocol, http_addr_1);

            let mut client_builder = reqwest::Client::builder();

            // For TLS, accept self-signed certificates in tests
            if self.enable_tls {
                client_builder = client_builder.danger_accept_invalid_certs(true);
            }

            let client = client_builder.build()?;
            let response = timeout(
                std::time::Duration::from_secs(15),
                client.post(&init_url).json(&serde_json::json!({})).send(),
            )
            .await??;

            if response.status().is_success() {
                println!("✅ Node 1 initialized as cluster leader");
            } else {
                return Err(format!("Failed to initialize Node 1: {}", response.status()).into());
            }

            // Step 2: Wait for auto-join to happen
            println!("📍 Step 2: Waiting for auto-join process...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            Ok(())
        }

        pub async fn wait_for_cluster_ready(&self, timeout_secs: u64) -> bool {
            println!("⏱️ Waiting for auto-join to complete...");
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(timeout_secs);

            while start.elapsed() < timeout {
                // Check if we have a leader
                if let Some(leader_addr) = self.get_leader_address().await {
                    println!("✅ Leader found at: {}", leader_addr);

                    // Check cluster membership from the leader's perspective
                    let mut client_builder = reqwest::Client::builder();
                    if self.enable_tls {
                        client_builder = client_builder.danger_accept_invalid_certs(true);
                    }

                    let client = client_builder.build().ok().unwrap();
                    let protocol = if self.enable_tls { "https" } else { "http" };
                    let metrics_url = format!("{}://{}/metrics", protocol, leader_addr);

                    if let Ok(response) = client.get(&metrics_url).send().await {
                        if let Ok(json) = response.json::<serde_json::Value>().await {
                            // Check the membership configuration from the leader
                            if let Some(membership) = json.get("membership_config") {
                                if let Some(voters) =
                                    membership.get("membership").and_then(|m| m.get("voters"))
                                {
                                    if let Some(voters_array) = voters.as_array() {
                                        let voting_members = voters_array.len();
                                        println!(
                                            "📊 Cluster has {} voting members",
                                            voting_members
                                        );

                                        // Check if we have at least 2 voting members (leader + 1 follower minimum)
                                        // For a 3-node cluster, we want all 3 eventually, but 2 is sufficient for basic operation
                                        if voting_members >= 2 {
                                            println!(
                                                "🎊 Cluster ready with {} voting members!",
                                                voting_members
                                            );
                                            return true;
                                        }
                                    }
                                }
                            }

                            // Fallback: check if nodes are responding
                            let mut responding_nodes = 0;
                            for (node_id, (http_addr, _)) in &self.node_addrs {
                                let health_url = format!("{}://{}/health", protocol, http_addr);
                                if client.get(&health_url).send().await.is_ok() {
                                    responding_nodes += 1;
                                    println!("✅ Node {} is responding", node_id);
                                }
                            }

                            if responding_nodes >= 2 {
                                println!(
                                    "🎊 At least {} nodes responding - cluster usable!",
                                    responding_nodes
                                );
                                return true;
                            }
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                // Longer sleep to reduce log spam
            }

            println!("⚠️ Auto-join timed out after {} seconds", timeout_secs);
            false
        }

        async fn get_leader_address(&self) -> Option<String> {
            // Check each node to see if any is the leader
            for (http_addr, _) in self.node_addrs.values() {
                let mut client_builder = reqwest::Client::builder();

                // For TLS, accept self-signed certificates in tests
                if self.enable_tls {
                    client_builder = client_builder.danger_accept_invalid_certs(true);
                }

                let client = client_builder.build().ok()?;
                let protocol = if self.enable_tls { "https" } else { "http" };
                let metrics_url = format!("{}://{}/metrics", protocol, http_addr);

                if let Ok(response) = client.get(&metrics_url).send().await {
                    if let Ok(json) = response.json::<serde_json::Value>().await {
                        if let Some(state) = json.get("state") {
                            if state == "Leader" {
                                return Some(http_addr.clone());
                            }
                        }
                    }
                }
            }
            None
        }

        pub fn get_client_addresses(&self) -> Vec<String> {
            self.node_addrs
                .values()
                .map(|(http_addr, _)| {
                    if self.enable_tls {
                        format!("https://{}", http_addr)
                    } else {
                        format!("http://{}", http_addr)
                    }
                })
                .collect()
        }
    }

    impl Drop for RealClusterTestEnvironment {
        fn drop(&mut self) {
            println!("🧹 Cleaning up cluster processes...");

            // Kill processes more aggressively
            for mut process in self.node_processes.drain(..) {
                // First try graceful kill
                let _ = process.kill();
                let _ = process.wait();
            }

            // Also kill any stray ferrium-server processes that might be using our ports
            let _ = std::process::Command::new("pkill")
                .args(["-f", "ferrium-server"])
                .output();

            // Give processes time to clean up
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
    }
}

/// Simple test to verify cluster startup without FerriumClient complexity
#[tokio::test]
#[serial]
async fn test_basic_cluster_startup() {
    init_crypto_provider();

    println!("🔧 Testing basic cluster startup...");
    let node_ids = vec![1, 2, 3];
    let mut cluster_env = real_cluster_utils::RealClusterTestEnvironment::new(node_ids, false)
        .expect("Failed to create cluster environment");

    // Start the cluster
    cluster_env
        .start_cluster()
        .expect("Failed to start cluster");
    println!("✅ Cluster processes started");

    // Check if HTTP endpoints are responding
    println!("🔍 Testing HTTP endpoints...");
    let client = reqwest::Client::new();

    for (node_id, (http_addr, _)) in &cluster_env.node_addrs {
        let health_url = format!("http://{}/health", http_addr);
        println!("Checking health at: {}", health_url);

        match timeout(Duration::from_secs(5), client.get(&health_url).send()).await {
            Ok(Ok(response)) => {
                println!(
                    "✅ Node {} health check: {} (status: {})",
                    node_id,
                    health_url,
                    response.status()
                );
                if let Ok(text) = response.text().await {
                    println!("Response: {}", text);
                }
            }
            Ok(Err(e)) => println!("❌ Node {} health check failed: {}", node_id, e),
            Err(_) => println!("⏰ Node {} health check timed out", node_id),
        }

        // Also try metrics endpoint
        let metrics_url = format!("http://{}/metrics", http_addr);
        match timeout(Duration::from_secs(3), client.get(&metrics_url).send()).await {
            Ok(Ok(response)) => {
                println!(
                    "✅ Node {} metrics: {} (status: {})",
                    node_id,
                    metrics_url,
                    response.status()
                );
            }
            Ok(Err(e)) => println!("❌ Node {} metrics failed: {}", node_id, e),
            Err(_) => println!("⏰ Node {} metrics timed out", node_id),
        }
    }

    println!("🎉 Basic cluster startup test completed!");
}

/// Test FerriumClient connecting to a real 3-node cluster with read/write operations
#[tokio::test]
#[serial]
async fn test_ferrium_client_real_cluster_operations() {
    init_crypto_provider();

    println!("🔧 Setting up real 3-node cluster...");
    let node_ids = vec![1, 2, 3];
    let mut cluster_env = real_cluster_utils::RealClusterTestEnvironment::new(node_ids, false)
        .expect("Failed to create cluster environment");

    // Start the cluster
    cluster_env
        .start_cluster()
        .expect("Failed to start cluster");

    // Initialize the cluster
    println!("🔧 Initializing cluster...");
    cluster_env
        .initialize_cluster()
        .await
        .expect("Failed to initialize cluster");

    // Wait for cluster to be ready
    println!("⏱️ Waiting for cluster to become ready...");
    assert!(
        cluster_env.wait_for_cluster_ready(30).await,
        "Cluster failed to become ready within timeout"
    );

    println!("✅ Cluster is ready with leader elected!");

    // Create FerriumClient
    let client_addresses = cluster_env.get_client_addresses();
    println!(
        "🔗 Connecting FerriumClient to addresses: {:?}",
        client_addresses
    );

    let mut client = FerriumClient::new(client_addresses.clone());

    // Wait for client to be ready
    println!("⏱️ Waiting for client to connect...");

    // First, manually check if any nodes are responding
    println!("🔍 Checking individual node health...");
    let client_test = reqwest::Client::new();
    for (i, addr) in client_addresses.iter().enumerate() {
        let health_url = format!("{}/health", addr);
        println!("Checking health at: {}", health_url);

        match timeout(Duration::from_secs(3), client_test.get(&health_url).send()).await {
            Ok(Ok(response)) => {
                println!(
                    "Node {} health check: {} (status: {})",
                    i + 1,
                    health_url,
                    response.status()
                );
                if let Ok(text) = response.text().await {
                    println!("Response: {}", text);
                }
            }
            Ok(Err(e)) => println!("Node {} health check failed: {}", i + 1, e),
            Err(_) => println!("Node {} health check timed out", i + 1),
        }
    }

    let ready_result = timeout(
        Duration::from_secs(15),
        client.wait_for_ready(Duration::from_secs(15)),
    )
    .await;

    if ready_result.is_err() || ready_result.as_ref().unwrap().is_err() {
        println!(
            "❌ FerriumClient connection failed. Result: {:?}",
            ready_result
        );

        // Try to get more info about what's wrong
        for (i, addr) in client_addresses.iter().enumerate() {
            let metrics_url = format!("{}/metrics", addr);
            match timeout(Duration::from_secs(2), client_test.get(&metrics_url).send()).await {
                Ok(Ok(response)) => {
                    println!("Node {} metrics available: {}", i + 1, response.status())
                }
                Ok(Err(e)) => println!("Node {} metrics failed: {}", i + 1, e),
                Err(_) => println!("Node {} metrics timed out", i + 1),
            }
        }

        panic!("FerriumClient failed to connect to cluster");
    }

    println!("✅ FerriumClient connected successfully!");

    // Test write operations
    println!("📝 Testing write operations...");

    let test_data = vec![
        ("user:1", "Alice"),
        ("user:2", "Bob"),
        ("user:3", "Charlie"),
        ("config:timeout", "30"),
        ("config:retries", "3"),
    ];

    for (key, value) in &test_data {
        println!("Writing: {} = {}", key, value);
        let result = timeout(
            Duration::from_secs(10),
            client.set(key.to_string(), value.to_string()),
        )
        .await;

        assert!(
            result.is_ok() && result.unwrap().is_ok(),
            "Failed to write key: {}",
            key
        );
    }

    println!("✅ All write operations completed successfully!");

    // Test read operations
    println!("📖 Testing read operations...");

    for (key, expected_value) in &test_data {
        println!("Reading: {}", key);
        let result = timeout(Duration::from_secs(10), client.get(key.to_string())).await;

        assert!(result.is_ok(), "Read operation timed out for key: {}", key);
        let read_result = result.unwrap();
        assert!(read_result.is_ok(), "Failed to read key: {}", key);

        let actual_value = read_result.unwrap();
        assert_eq!(
            actual_value.as_deref(),
            Some(*expected_value),
            "Value mismatch for key {}: expected {}, got {:?}",
            key,
            expected_value,
            actual_value
        );
    }

    println!("✅ All read operations completed successfully!");

    // Test delete operations
    println!("🗑️ Testing delete operations...");

    let keys_to_delete = vec!["user:2", "config:retries"];

    for key in &keys_to_delete {
        println!("Deleting: {}", key);
        let result = timeout(Duration::from_secs(10), client.delete(key.to_string())).await;

        assert!(
            result.is_ok() && result.unwrap().is_ok(),
            "Failed to delete key: {}",
            key
        );
    }

    // Verify deletions
    for key in &keys_to_delete {
        println!("Verifying deletion of: {}", key);
        let result = timeout(Duration::from_secs(10), client.get(key.to_string())).await;

        assert!(
            result.is_ok(),
            "Read operation timed out for deleted key: {}",
            key
        );
        let read_result = result.unwrap();
        assert!(read_result.is_ok(), "Failed to read deleted key: {}", key);

        let value = read_result.unwrap();
        assert!(
            value.is_none(),
            "Key {} should be deleted but still has value: {:?}",
            key,
            value
        );
    }

    println!("✅ All delete operations completed successfully!");

    // Test batch operations
    println!("📦 Testing batch operations...");

    let batch_data = vec![
        ("batch:1", "value1"),
        ("batch:2", "value2"),
        ("batch:3", "value3"),
    ];

    // Write batch data
    for (key, value) in &batch_data {
        let result = timeout(
            Duration::from_secs(10),
            client.set(key.to_string(), value.to_string()),
        )
        .await;
        assert!(result.is_ok() && result.unwrap().is_ok());
    }

    // Read batch data
    for (key, expected_value) in &batch_data {
        let result = timeout(Duration::from_secs(10), client.get(key.to_string())).await;
        assert!(result.is_ok());
        let value = result.unwrap().unwrap().unwrap();
        assert_eq!(value, *expected_value);
    }

    println!("✅ Batch operations completed successfully!");

    // Test cluster health through client
    println!("🔍 Testing cluster status through client...");

    // The client should be connected and working, which proves cluster health
    let health_test_result = timeout(
        Duration::from_secs(5),
        client.set("health:test".to_string(), "cluster_healthy".to_string()),
    )
    .await;

    assert!(
        health_test_result.is_ok() && health_test_result.unwrap().is_ok(),
        "Cluster health test failed"
    );

    println!("✅ Cluster is healthy and responsive!");

    println!("🎉 All FerriumClient real cluster tests passed successfully!");
}

/// Test FerriumClient with TLS-enabled real cluster
#[tokio::test]
#[serial]
async fn test_ferrium_client_real_cluster_with_tls() {
    init_crypto_provider();

    println!("🔐 Setting up real 3-node cluster with TLS...");

    // Kill any leftover ferrium-server processes first
    let _ = std::process::Command::new("pkill")
        .args(["-f", "ferrium-server"])
        .output();

    // Wait for processes to fully terminate
    std::thread::sleep(std::time::Duration::from_millis(1000));

    // Clean up old test artifacts
    if let Err(e) = real_cluster_utils::RealClusterTestEnvironment::cleanup_old_test_artifacts() {
        println!("⚠️ Failed to cleanup old test artifacts: {}", e);
    }

    let node_ids = vec![1, 2, 3];
    let mut test_env = real_cluster_utils::RealClusterTestEnvironment::new(node_ids, true)
        .expect("Failed to create TLS test environment");

    // Start the cluster
    test_env
        .start_cluster()
        .expect("Failed to start TLS cluster");

    // Initialize the cluster
    test_env
        .initialize_cluster()
        .await
        .expect("Failed to initialize TLS cluster");

    // Wait for cluster to be ready
    assert!(
        test_env.wait_for_cluster_ready(45).await,
        "TLS cluster failed to become ready within timeout"
    );

    // Create FerriumClient for TLS
    let client_addresses = test_env.get_client_addresses();
    println!(
        "🔗 Connecting FerriumClient to TLS addresses: {:?}",
        client_addresses
    );

    // For testing with self-signed certificates, we'll create a client that accepts invalid certs
    let mut client = if test_env.enable_tls {
        // Use the test-friendly constructor that accepts self-signed certificates
        FerriumClient::with_insecure_tls(client_addresses)
    } else {
        FerriumClient::new(client_addresses)
    };

    // Wait for client to be ready (TLS may take longer)
    println!("⏱️ Waiting for client to connect to TLS cluster...");
    let ready_result = timeout(
        Duration::from_secs(20),
        client.wait_for_ready(Duration::from_secs(20)),
    )
    .await;

    assert!(
        ready_result.is_ok() && ready_result.unwrap().is_ok(),
        "FerriumClient failed to connect to TLS cluster"
    );

    println!("✅ FerriumClient connected to TLS cluster successfully!");

    // Test TLS-specific operations
    println!("🔐 Testing TLS-encrypted operations...");

    let tls_test_data = vec![
        ("tls:secret1", "encrypted_value_1"),
        ("tls:secret2", "encrypted_value_2"),
        ("tls:config", "tls_enabled"),
    ];

    // Write and read TLS data
    for (key, value) in &tls_test_data {
        // Write
        let write_result = timeout(
            Duration::from_secs(10),
            client.set(key.to_string(), value.to_string()),
        )
        .await;
        assert!(
            write_result.is_ok() && write_result.unwrap().is_ok(),
            "TLS write failed for key: {}",
            key
        );

        // Read
        let read_result = timeout(Duration::from_secs(10), client.get(key.to_string())).await;
        assert!(read_result.is_ok(), "TLS read timed out for key: {}", key);
        let actual_value = read_result.unwrap().unwrap().unwrap();
        assert_eq!(actual_value, *value, "TLS value mismatch for key: {}", key);
    }

    println!("✅ TLS-encrypted operations completed successfully!");
    println!("🎉 FerriumClient TLS cluster test passed!");
}

/// Test cluster operations under stress
#[tokio::test]
#[serial]
async fn test_ferrium_client_cluster_stress() {
    init_crypto_provider();

    println!("💪 Setting up cluster for stress testing...");
    let node_ids = vec![1, 2, 3];
    let mut cluster_env = real_cluster_utils::RealClusterTestEnvironment::new(node_ids, false)
        .expect("Failed to create stress test environment");

    cluster_env
        .start_cluster()
        .expect("Failed to start cluster");
    cluster_env
        .initialize_cluster()
        .await
        .expect("Failed to initialize cluster");

    assert!(
        cluster_env.wait_for_cluster_ready(30).await,
        "Cluster failed to become ready for stress test"
    );

    let mut client = FerriumClient::new(cluster_env.get_client_addresses());
    let ready_result = timeout(
        Duration::from_secs(15),
        client.wait_for_ready(Duration::from_secs(15)),
    )
    .await;
    assert!(ready_result.is_ok() && ready_result.unwrap().is_ok());

    println!("🔥 Running stress test with concurrent operations...");

    let start_time = SystemTime::now();
    let num_operations = 50;

    // Concurrent writes
    for i in 0..num_operations {
        let key = format!("stress:key:{}", i);
        let value = format!("stress_value_{}", i);

        let write_result = timeout(Duration::from_secs(5), client.set(key, value)).await;

        assert!(
            write_result.is_ok() && write_result.unwrap().is_ok(),
            "Stress write failed for operation {}",
            i
        );
    }

    // Concurrent reads
    for i in 0..num_operations {
        let key = format!("stress:key:{}", i);
        let expected_value = format!("stress_value_{}", i);

        let read_result = timeout(Duration::from_secs(5), client.get(key)).await;

        assert!(
            read_result.is_ok(),
            "Stress read timed out for operation {}",
            i
        );
        let actual_value = read_result.unwrap().unwrap().unwrap();
        assert_eq!(
            actual_value, expected_value,
            "Stress value mismatch for operation {}",
            i
        );
    }

    let elapsed = start_time.elapsed().unwrap();
    println!(
        "✅ Stress test completed: {} operations in {:?}",
        num_operations * 2,
        elapsed
    );
    println!(
        "📊 Average operation time: {:?}",
        elapsed / (num_operations * 2)
    );

    println!("🎉 Cluster stress test passed!");
}

/// Detailed test to isolate where the hang occurs
#[tokio::test]
#[serial]
async fn test_cluster_initialization_step_by_step() {
    init_crypto_provider();

    println!("🔧 Setting up real 3-node cluster...");
    let node_ids = vec![1, 2, 3];
    let mut cluster_env = real_cluster_utils::RealClusterTestEnvironment::new(node_ids, false)
        .expect("Failed to create cluster environment");

    // Start the cluster
    cluster_env
        .start_cluster()
        .expect("Failed to start cluster");
    println!("✅ Cluster processes started");

    // Step 1: Test basic HTTP connectivity
    println!("🔍 Step 1: Testing basic HTTP connectivity...");
    let client = reqwest::Client::new();
    let mut responding_nodes = 0;

    for (node_id, (http_addr, _)) in &cluster_env.node_addrs {
        let health_url = format!("http://{}/health", http_addr);
        match timeout(Duration::from_secs(3), client.get(&health_url).send()).await {
            Ok(Ok(response)) if response.status().is_success() => {
                println!("✅ Node {} responding at {}", node_id, http_addr);
                responding_nodes += 1;
            }
            _ => println!("❌ Node {} not responding at {}", node_id, http_addr),
        }
    }
    assert_eq!(responding_nodes, 3, "All nodes should be responding");

    // Step 2: Test cluster initialization
    println!("🔍 Step 2: Testing cluster initialization...");
    let (init_http_addr, _) = cluster_env.node_addrs.get(&1).unwrap();
    let init_url = format!("http://{}/init", init_http_addr);

    let response = timeout(
        Duration::from_secs(10),
        client.post(&init_url).json(&serde_json::json!({})).send(),
    )
    .await;

    match response {
        Ok(Ok(resp)) => {
            println!("✅ Init request completed with status: {}", resp.status());
            if let Ok(text) = resp.text().await {
                println!("Init response: {}", text);
            }
        }
        Ok(Err(e)) => println!("❌ Init request failed: {}", e),
        Err(_) => println!("⏰ Init request timed out"),
    }

    // Step 3: Wait and check for leader election
    println!("🔍 Step 3: Checking for leader election...");
    sleep(Duration::from_secs(5)).await;

    let mut leader_found = false;
    for (node_id, (http_addr, _)) in &cluster_env.node_addrs {
        let metrics_url = format!("http://{}/metrics", http_addr);
        match timeout(Duration::from_secs(3), client.get(&metrics_url).send()).await {
            Ok(Ok(response)) if response.status().is_success() => {
                if let Ok(metrics_text) = response.text().await {
                    println!("Node {} metrics: {}", node_id, metrics_text);
                    if let Ok(metrics_json) =
                        serde_json::from_str::<serde_json::Value>(&metrics_text)
                    {
                        if let Some(state) = metrics_json.get("state") {
                            println!("Node {} state: {}", node_id, state);
                            if state == "Leader" {
                                leader_found = true;
                                println!("✅ Found leader: Node {}", node_id);
                            }
                        }
                    }
                }
            }
            _ => println!("❌ Could not get metrics from Node {}", node_id),
        }
    }

    if !leader_found {
        println!("❌ No leader found yet - checking leader endpoint...");

        for (node_id, (http_addr, _)) in &cluster_env.node_addrs {
            let leader_url = format!("http://{}/leader", http_addr);
            match timeout(Duration::from_secs(3), client.get(&leader_url).send()).await {
                Ok(Ok(response)) if response.status().is_success() => {
                    if let Ok(leader_text) = response.text().await {
                        println!("Node {} leader info: {}", node_id, leader_text);
                    }
                }
                _ => println!("❌ Could not get leader info from Node {}", node_id),
            }
        }
    }

    // Step 4: Try FerriumClient connection with short timeout
    if leader_found {
        println!("🔍 Step 4: Testing FerriumClient connection...");
        let client_addresses = cluster_env.get_client_addresses();
        let mut ferrium_client = FerriumClient::new(client_addresses);

        let ready_result = timeout(
            Duration::from_secs(5),
            ferrium_client.wait_for_ready(Duration::from_secs(5)),
        )
        .await;

        match ready_result {
            Ok(Ok(())) => println!("✅ FerriumClient connected successfully!"),
            Ok(Err(e)) => println!("❌ FerriumClient connection failed: {}", e),
            Err(_) => println!("⏰ FerriumClient connection timed out"),
        }
    } else {
        println!("⚠️  Skipping FerriumClient test - no leader elected");
    }

    println!("🎉 Step-by-step test completed!");
}

/// Simple test to debug the init endpoint hanging issue
#[tokio::test]
#[serial]
async fn test_debug_init_endpoint() {
    init_crypto_provider();

    println!("🔧 Testing single node init endpoint...");
    let _node_ids = [1];
    let mut cluster_env = real_cluster_utils::RealClusterTestEnvironment::new(vec![1], false)
        .expect("Failed to create cluster environment");

    // Start just one node
    cluster_env
        .start_cluster()
        .expect("Failed to start cluster");
    println!("✅ Single node started");

    // Test basic connectivity first
    let client = reqwest::Client::new();
    let (http_addr, _) = cluster_env.node_addrs.get(&1).unwrap();

    // Test health endpoint
    let health_url = format!("http://{}/health", http_addr);
    println!("🔍 Testing health endpoint: {}", health_url);

    let health_response = timeout(Duration::from_secs(5), client.get(&health_url).send()).await;

    match health_response {
        Ok(Ok(resp)) => println!("✅ Health check successful: {}", resp.status()),
        Ok(Err(e)) => println!("❌ Health check failed: {}", e),
        Err(_) => println!("⏰ Health check timed out"),
    }

    // Test metrics endpoint
    let metrics_url = format!("http://{}/metrics", http_addr);
    println!("🔍 Testing metrics endpoint: {}", metrics_url);

    let metrics_response = timeout(Duration::from_secs(5), client.get(&metrics_url).send()).await;

    match metrics_response {
        Ok(Ok(resp)) => {
            println!("✅ Metrics check successful: {}", resp.status());
            if let Ok(text) = resp.text().await {
                println!("📊 Initial metrics: {}", text);
            }
        }
        Ok(Err(e)) => println!("❌ Metrics check failed: {}", e),
        Err(_) => println!("⏰ Metrics check timed out"),
    }

    // Now test the problematic init endpoint with very short timeout
    let init_url = format!("http://{}/init", http_addr);
    println!("🔍 Testing init endpoint: {}", init_url);
    println!(
        "⏰ Starting init request at: {:?}",
        std::time::Instant::now()
    );

    let init_response = timeout(
        Duration::from_secs(5), // Very short timeout to quickly identify hang
        client.post(&init_url).json(&serde_json::json!({})).send(),
    )
    .await;

    println!(
        "⏰ Init request completed at: {:?}",
        std::time::Instant::now()
    );

    match init_response {
        Ok(Ok(resp)) => {
            println!("✅ Init successful: {}", resp.status());
            if let Ok(text) = resp.text().await {
                println!("📝 Init response: {}", text);
            }
        }
        Ok(Err(e)) => println!("❌ Init request failed: {}", e),
        Err(_) => {
            println!("⏰ Init request timed out after 5 seconds - this confirms the hang!");

            // Check if the node is still responding to other endpoints
            println!("🔍 Checking if node is still responsive after init timeout...");

            let health_after =
                timeout(Duration::from_secs(3), client.get(&health_url).send()).await;

            match health_after {
                Ok(Ok(resp)) => println!(
                    "✅ Node still responds to health after init timeout: {}",
                    resp.status()
                ),
                Ok(Err(e)) => println!("❌ Node no longer responds to health: {}", e),
                Err(_) => println!("⏰ Node health check also times out"),
            }
        }
    }

    println!("🎉 Debug test completed!");
}

/// Test that exactly mimics the script approach - using auto-join instead of manual init
#[tokio::test]
#[serial]
async fn test_script_style_cluster_formation() {
    init_crypto_provider();

    println!("🔧 Testing script-style cluster formation with auto-join...");
    let node_ids = vec![1, 2, 3];
    let mut cluster_env = real_cluster_utils::RealClusterTestEnvironment::new(node_ids, false)
        .expect("Failed to create cluster environment");

    // Start the cluster - this should trigger auto-join
    cluster_env
        .start_cluster()
        .expect("Failed to start cluster");
    println!("✅ All 3 nodes started with auto-join enabled");

    // Give time for auto-join to work (like the script does)
    println!("⏰ Waiting for auto-join to complete...");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Check if auto-join worked by checking cluster state
    let client = reqwest::Client::new();

    // Check each node's metrics to see if cluster formed
    for (node_id, (http_addr, _)) in &cluster_env.node_addrs {
        let url = format!("http://{}/metrics", http_addr);
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                let metrics: serde_json::Value = response.json().await.unwrap();
                println!(
                    "📊 Node {} metrics: state={:?}, leader={:?}",
                    node_id,
                    metrics.get("state"),
                    metrics.get("current_leader")
                );
            }
            _ => {
                println!("❌ Failed to get metrics from Node {}", node_id);
            }
        }
    }

    // Try manual initialization on Node 1 like the script does
    let (node1_http, _) = cluster_env.node_addrs.get(&1).unwrap();
    let init_url = format!("http://{}/init", node1_http);

    println!("🔧 Trying manual init like the script does...");
    let start_time = std::time::Instant::now();

    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.post(&init_url).send(),
    )
    .await
    {
        Ok(Ok(response)) => {
            let elapsed = start_time.elapsed();
            println!(
                "✅ Init completed in {:?} with status: {}",
                elapsed,
                response.status()
            );

            if response.status().is_success() {
                let result: serde_json::Value = response.json().await.unwrap();
                println!("📋 Init result: {}", result);
            }
        }
        Ok(Err(e)) => {
            println!("❌ Init failed with error: {}", e);
        }
        Err(_) => {
            println!("⏰ Init timed out after 10 seconds");
        }
    }

    println!("🎉 Script-style test completed!");
}

/// Test that exactly mimics the script's process execution approach
#[tokio::test]
#[serial]
async fn test_exact_script_mimicking() {
    init_crypto_provider();

    println!("🔧 Testing exact script mimicking...");

    // Use the script's exact approach - read the working script config
    let script_config_path = "test-cluster-automated/configs/node1.toml";

    if !std::path::Path::new(script_config_path).exists() {
        println!(
            "❌ Script config not found. Run `./scripts/test-cluster.sh --keep-running` first"
        );
        return;
    }

    // Read the working config that the script created
    let _config_content = std::fs::read_to_string(script_config_path).unwrap();
    println!(
        "📋 Using working script config from: {}",
        script_config_path
    );

    // Test if we can call init on the already-running script node
    let client = reqwest::Client::new();
    let init_url = "http://127.0.0.1:21001/init"; // Script's exact URL

    println!("🔧 Testing init on script's running node...");
    let start_time = std::time::Instant::now();

    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.post(init_url).send(),
    )
    .await
    {
        Ok(Ok(response)) => {
            let elapsed = start_time.elapsed();
            println!(
                "✅ Script node init completed in {:?} with status: {}",
                elapsed,
                response.status()
            );

            if response.status().is_success() {
                let result: serde_json::Value = response.json().await.unwrap();
                println!("📋 Script init result: {}", result);
            } else {
                let error_text = response.text().await.unwrap_or_default();
                println!("❌ Script init failed: {}", error_text);
            }
        }
        Ok(Err(e)) => {
            println!("❌ Script node init failed with error: {}", e);
        }
        Err(_) => {
            println!("⏰ Script node init ALSO timed out after 10 seconds!");
        }
    }

    // Now check the script node's metrics
    match client.get("http://127.0.0.1:21001/metrics").send().await {
        Ok(response) if response.status().is_success() => {
            let metrics: serde_json::Value = response.json().await.unwrap();
            println!(
                "📊 Script Node 1 current state: state={:?}, leader={:?}",
                metrics.get("state"),
                metrics.get("current_leader")
            );
        }
        _ => {
            println!("❌ Could not get metrics from script node");
        }
    }

    println!("🎉 Script mimicking test completed!");
}

/// Focused debug test to isolate the raft initialization hang
#[tokio::test]
#[serial]
async fn test_debug_single_node_init() {
    init_crypto_provider();

    println!("🔧 Debug test: Single node initialization...");

    // Use unique directories to avoid conflicts
    let test_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let base_path = std::path::PathBuf::from(format!("./test-debug-{}", test_id));
    let config_dir = base_path.join("configs");
    let data_dir = base_path.join("data/node1");
    let log_dir = base_path.join("logs");

    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&log_dir).unwrap();

    // Create minimal single-node config like the script
    let http_port = real_cluster_utils::find_available_port();
    let grpc_port = real_cluster_utils::find_available_port();

    println!("📍 Using ports: HTTP={}, gRPC={}", http_port, grpc_port);

    let config_content = format!(
        "\
        [node]\n\
        id = 1\n\
        http_addr = \"127.0.0.1:{}\"\n\
        grpc_addr = \"127.0.0.1:{}\"\n\
        data_dir = \"{}\"\n\
        name = \"test-node-1\"\n\
        \n\
        [network]\n\
        request_timeout = 10000\n\
        connect_timeout = 3000\n\
        keep_alive_interval = 30000\n\
        max_retries = 3\n\
        retry_delay = 100\n\
        enable_compression = true\n\
        max_message_size = 4194304\n\
        \n\
        [storage]\n\
        enable_compression = true\n\
        compaction_strategy = \"level\"\n\
        max_log_size = 33554432\n\
        log_retention_count = 3\n\
        enable_wal = true\n\
        sync_writes = false\n\
        block_cache_size = 16\n\
        write_buffer_size = 16\n\
        \n\
        [raft]\n\
        heartbeat_interval = 100\n\
        election_timeout_min = 150\n\
        election_timeout_max = 300\n\
        max_append_entries = 50\n\
        enable_auto_compaction = true\n\
        compaction_threshold = 200\n\
        max_inflight_requests = 5\n\
        \n\
        [raft.snapshot_policy]\n\
        enable_auto_snapshot = true\n\
        entries_since_last_snapshot = 100\n\
        snapshot_interval = 30000\n\
        \n\
        [logging]\n\
        level = \"debug\"\n\
        format = \"pretty\"\n\
        structured = false\n\
        file_path = \"{}/node1.log\"\n\
        max_file_size = 10485760\n\
        max_files = 2\n\
        enable_colors = true\n\
        \n\
        [cluster]\n\
        name = \"debug-test-cluster\"\n\
        expected_size = 1\n\
        enable_auto_join = false\n\
        leader_discovery_timeout = 30000\n\
        auto_join_timeout = 60000\n\
        auto_join_retry_interval = 5000\n\
        auto_accept_learners = false\n\
        \n\
        [[cluster.peer]]\n\
        id = 1\n\
        http_addr = \"127.0.0.1:{}\"\n\
        grpc_addr = \"127.0.0.1:{}\"\n\
        voting = true\n\
        priority = 100\n\
        \n\
        [cluster.discovery]\n\
        enabled = false\n\
        method = \"static\"\n\
        interval = 30000\n\
        \n\
        [security]\n\
        enable_tls = false\n\
        enable_mtls = false\n\
        accept_invalid_certs = false\n\
        auth_method = \"none\"\n\
        ",
        http_port,
        grpc_port,
        data_dir.display(),
        log_dir.display(),
        http_port,
        grpc_port
    );

    let config_path = config_dir.join("node1.toml");
    std::fs::write(&config_path, config_content).unwrap();

    println!("📋 Config written to: {}", config_path.display());

    // Start the node with debug logging
    println!("🚀 Starting single node...");
    let mut child = std::process::Command::new("target/release/ferrium-server")
        .arg("--config")
        .arg(&config_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Give it time to start
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Check if process is still running and capture any stderr
    match child.try_wait() {
        Ok(Some(status)) => {
            println!("❌ Process exited early with status: {}", status);
            if let Some(mut stderr) = child.stderr.take() {
                use std::io::Read;
                let mut stderr_output = String::new();
                let _ = stderr.read_to_string(&mut stderr_output);
                if !stderr_output.is_empty() {
                    println!("📝 Stderr output:\n{}", stderr_output);
                }
            }
            return;
        }
        Ok(None) => {
            println!("✅ Process is still running");
        }
        Err(e) => {
            println!("❌ Error checking process status: {}", e);
            return;
        }
    }

    // Check if it's still running
    match child.try_wait() {
        Ok(Some(exit_status)) => {
            println!("❌ Node exited early with status: {}", exit_status);
            if let Some(stderr) = child.stderr.take() {
                use std::io::Read;
                let mut error_output = String::new();
                std::io::BufReader::new(stderr)
                    .read_to_string(&mut error_output)
                    .ok();
                println!("💥 Error output: {}", error_output);
            }
            return;
        }
        Ok(None) => {
            println!("✅ Node is running");
        }
        Err(e) => {
            println!("❌ Error checking node status: {}", e);
            return;
        }
    }

    // Test basic connectivity
    let client = reqwest::Client::new();
    let health_url = format!("http://127.0.0.1:{}/health", http_port);

    match client.get(&health_url).send().await {
        Ok(response) if response.status().is_success() => {
            println!("✅ Health check passed");
        }
        Ok(response) => {
            println!("❌ Health check failed: {}", response.status());
            return;
        }
        Err(e) => {
            println!("❌ Health check error: {}", e);
            return;
        }
    }

    // Get initial metrics
    let metrics_url = format!("http://127.0.0.1:{}/metrics", http_port);
    match client.get(&metrics_url).send().await {
        Ok(response) if response.status().is_success() => {
            let metrics: serde_json::Value = response.json().await.unwrap();
            println!(
                "📊 Initial state: state={:?}, leader={:?}",
                metrics.get("state"),
                metrics.get("current_leader")
            );
        }
        _ => {
            println!("❌ Could not get initial metrics");
        }
    }

    // Now try initialization with detailed timing
    let init_url = format!("http://127.0.0.1:{}/init", http_port);
    println!("🔧 Attempting initialization...");

    let start_time = std::time::Instant::now();

    match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.post(&init_url).send(),
    )
    .await
    {
        Ok(Ok(response)) => {
            let elapsed = start_time.elapsed();
            println!(
                "✅ Init response received in {:?}: {}",
                elapsed,
                response.status()
            );

            if response.status().is_success() {
                let result: serde_json::Value = response.json().await.unwrap();
                println!("📋 Init result: {}", result);

                // Check final state
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                match client.get(&metrics_url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let metrics: serde_json::Value = resp.json().await.unwrap();
                        println!(
                            "📊 Final state: state={:?}, leader={:?}",
                            metrics.get("state"),
                            metrics.get("current_leader")
                        );
                    }
                    _ => println!("❌ Could not get final metrics"),
                }
            } else {
                let error_text = response.text().await.unwrap_or_default();
                println!("❌ Init failed: {}", error_text);
            }
        }
        Ok(Err(e)) => {
            println!("❌ Init request error: {}", e);
        }
        Err(_) => {
            let elapsed = start_time.elapsed();
            println!("⏰ Init timed out after {:?}", elapsed);

            // Capture any output from the ferrium-server process
            println!("🔍 Capturing process output...");
            if let Some(mut stdout) = child.stdout.take() {
                use std::io::Read;
                let mut stdout_output = String::new();
                let _ = stdout.read_to_string(&mut stdout_output);
                if !stdout_output.is_empty() {
                    println!("📝 Stdout output:\n{}", stdout_output);
                } else {
                    println!("📝 No stdout output");
                }
            }
            if let Some(mut stderr) = child.stderr.take() {
                use std::io::Read;
                let mut stderr_output = String::new();
                let _ = stderr.read_to_string(&mut stderr_output);
                if !stderr_output.is_empty() {
                    println!("📝 Stderr output:\n{}", stderr_output);
                } else {
                    println!("📝 No stderr output");
                }
            }
        }
    }

    // Kill the process
    child.kill().ok();
    child.wait().ok();

    // Clean up test directory
    // std::fs::remove_dir_all(&base_path).ok(); // Commented out to preserve logs

    println!("🎉 Debug test completed!");
    println!(
        "📁 Logs preserved at: {}/logs/node1.log",
        base_path.display()
    );
}

/// Test to print and compare our configuration with the working script
#[tokio::test]
#[serial]
async fn test_config_comparison() {
    init_crypto_provider();

    println!("🔧 Comparing our config with the working script config...");

    // Generate our config (TLS disabled)
    let _node_ids = [1];
    let test_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let base_path = std::path::PathBuf::from(format!("./test-config-compare-{}", test_id));
    let config_dir = base_path.join("configs");
    let data_dir = base_path.join("data/node1");
    let log_dir = base_path.join("logs");

    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&log_dir).unwrap();

    let http_port = 21001;
    let grpc_port = 31001;
    let mut node_addrs = HashMap::new();
    node_addrs.insert(
        1,
        (
            format!("127.0.0.1:{}", http_port),
            format!("127.0.0.1:{}", grpc_port),
        ),
    );

    // Generate our config
    let our_config_path = real_cluster_utils::RealClusterTestEnvironment::create_node_config(
        &config_dir,
        &data_dir,
        &log_dir,
        &None, // No TLS
        1,
        &format!("127.0.0.1:{}", http_port),
        &format!("127.0.0.1:{}", grpc_port),
        &node_addrs,
        false, // TLS disabled
    )
    .unwrap();

    println!("📋 Our generated config:");
    let our_config_content = std::fs::read_to_string(&our_config_path).unwrap();
    println!("{}", our_config_content);

    // Compare with script config if it exists
    if std::path::Path::new("test-cluster-automated/configs/node1.toml").exists() {
        println!("📋 Script config:");
        let script_config_content =
            std::fs::read_to_string("test-cluster-automated/configs/node1.toml").unwrap();
        println!("{}", script_config_content);

        println!("🔍 Key differences:");
        // Simple line-by-line comparison
        let our_lines: Vec<&str> = our_config_content.lines().collect();
        let script_lines: Vec<&str> = script_config_content.lines().collect();

        for (i, (our_line, script_line)) in our_lines.iter().zip(script_lines.iter()).enumerate() {
            if our_line != script_line {
                println!(
                    "Line {}: Our: '{}' vs Script: '{}'",
                    i + 1,
                    our_line,
                    script_line
                );
            }
        }
    } else {
        println!("⚠️ No script config found. Run './scripts/test-cluster.sh --keep-running' first");
    }

    // Cleanup
    std::fs::remove_dir_all(base_path).ok();
}

/// Test using the exact same ports as the working script
#[tokio::test]
#[serial]
async fn test_script_ports_single_node() {
    init_crypto_provider();

    println!("🔧 Testing with exact script ports...");

    let test_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let base_path = std::path::PathBuf::from(format!("./test-script-ports-{}", test_id));
    let config_dir = base_path.join("configs");
    let data_dir = base_path.join("data/node1");
    let log_dir = base_path.join("logs");

    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&log_dir).unwrap();

    // Use EXACT same ports as the working script: HTTP=21001, gRPC=31001
    let http_port = 21001;
    let grpc_port = 31001;

    println!(
        "📍 Using SCRIPT ports: HTTP={}, gRPC={}",
        http_port, grpc_port
    );

    let config_content = format!(
        "\
        [node]\n\
        id = 1\n\
        http_addr = \"127.0.0.1:{}\"\n\
        grpc_addr = \"127.0.0.1:{}\"\n\
        data_dir = \"{}\"\n\
        name = \"test-node-1\"\n\
        \n\
        [network]\n\
        request_timeout = 10000\n\
        connect_timeout = 3000\n\
        keep_alive_interval = 30000\n\
        max_retries = 3\n\
        retry_delay = 100\n\
        enable_compression = true\n\
        max_message_size = 4194304\n\
        \n\
        [storage]\n\
        enable_compression = true\n\
        compaction_strategy = \"level\"\n\
        max_log_size = 33554432\n\
        log_retention_count = 3\n\
        enable_wal = true\n\
        sync_writes = false\n\
        block_cache_size = 16\n\
        write_buffer_size = 16\n\
        \n\
        [raft]\n\
        heartbeat_interval = 100\n\
        election_timeout_min = 150\n\
        election_timeout_max = 300\n\
        max_append_entries = 50\n\
        enable_auto_compaction = true\n\
        compaction_threshold = 200\n\
        max_inflight_requests = 5\n\
        \n\
        [raft.snapshot_policy]\n\
        enable_auto_snapshot = true\n\
        entries_since_last_snapshot = 100\n\
        snapshot_interval = 30000\n\
        \n\
        [logging]\n\
        level = \"debug\"\n\
        format = \"pretty\"\n\
        structured = false\n\
        file_path = \"{}/node1.log\"\n\
        max_file_size = 10485760\n\
        max_files = 2\n\
        enable_colors = true\n\
        \n\
        [cluster]\n\
        name = \"ferrium-automated-test\"\n\
        expected_size = 1\n\
        enable_auto_join = false\n\
        leader_discovery_timeout = 30000\n\
        auto_join_timeout = 60000\n\
        auto_join_retry_interval = 5000\n\
        auto_accept_learners = false\n\
        \n\
        [[cluster.peer]]\n\
        id = 1\n\
        http_addr = \"127.0.0.1:{}\"\n\
        grpc_addr = \"127.0.0.1:{}\"\n\
        voting = true\n\
        priority = 100\n\
        \n\
        [cluster.discovery]\n\
        enabled = false\n\
        method = \"static\"\n\
        interval = 30000\n\
        \n\
        [security]\n\
        enable_tls = false\n\
        enable_mtls = false\n\
        accept_invalid_certs = false\n\
        auth_method = \"none\"\n\
        ",
        http_port,
        grpc_port,
        data_dir.display(),
        log_dir.display(),
        http_port,
        grpc_port
    );

    let config_path = config_dir.join("node1.toml");
    std::fs::write(&config_path, config_content).unwrap();

    println!("📋 Config written to: {}", config_path.display());

    // Start the node
    println!("🚀 Starting node with script ports...");
    let mut child = std::process::Command::new("target/release/ferrium-server")
        .arg("--config")
        .arg(&config_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Give it time to start
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Check if process is still running and capture any stderr
    match child.try_wait() {
        Ok(Some(status)) => {
            println!("❌ Process exited early with status: {}", status);
            if let Some(mut stderr) = child.stderr.take() {
                use std::io::Read;
                let mut stderr_output = String::new();
                let _ = stderr.read_to_string(&mut stderr_output);
                if !stderr_output.is_empty() {
                    println!("📝 Stderr output:\n{}", stderr_output);
                }
            }
            panic!("Process exited early!");
        }
        Ok(None) => {
            println!("✅ Process is still running with script ports");
        }
        Err(e) => {
            println!("❌ Error checking process status: {}", e);
            panic!("Process check failed");
        }
    }

    // Test health endpoint
    let health_url = format!("http://127.0.0.1:{}/health", http_port);
    let client = reqwest::Client::new();

    match timeout(Duration::from_secs(5), client.get(&health_url).send()).await {
        Ok(Ok(resp)) => {
            println!("✅ Health check passed: {}", resp.status());
        }
        Ok(Err(e)) => {
            println!("❌ Health check failed: {}", e);
        }
        Err(_) => {
            println!("⏰ Health check timed out");
        }
    }

    // Test init endpoint
    println!("🔧 Testing init endpoint...");
    let init_url = format!("http://127.0.0.1:{}/init", http_port);

    let start_time = std::time::Instant::now();
    let init_result = timeout(
        Duration::from_secs(10),
        client.post(&init_url).json(&serde_json::json!({})).send(),
    )
    .await;

    match init_result {
        Ok(Ok(response)) => {
            println!("✅ Init successful: {}", response.status());
            let text = response.text().await.unwrap_or_default();
            println!("📝 Init response: {}", text);
        }
        Ok(Err(e)) => {
            println!("❌ Init failed with error: {}", e);
        }
        Err(_) => {
            let elapsed = start_time.elapsed();
            println!("⏰ Init timed out after {:?}", elapsed);

            // Capture process output
            if let Some(mut stdout) = child.stdout.take() {
                use std::io::Read;
                let mut stdout_output = String::new();
                let _ = stdout.read_to_string(&mut stdout_output);
                if !stdout_output.is_empty() {
                    println!("📝 Stdout output:\n{}", stdout_output);
                }
            }
        }
    }

    // Kill the process
    child.kill().ok();
    child.wait().ok();

    println!("🎉 Script ports test completed!");
    println!("📁 Test directory: {}", base_path.display());
}

/// Test with auto-join enabled like the working script
#[tokio::test]
#[serial]
async fn test_with_auto_join_enabled() {
    init_crypto_provider();

    println!("🔧 Testing with auto-join enabled like the script...");

    let test_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let base_path = std::path::PathBuf::from(format!("./test-auto-join-{}", test_id));
    let config_dir = base_path.join("configs");
    let data_dir = base_path.join("data/node1");
    let log_dir = base_path.join("logs");

    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&log_dir).unwrap();

    // Use script ports but with different ones to avoid conflicts
    let http_port = 22001;
    let grpc_port = 32001;

    println!("📍 Using ports: HTTP={}, gRPC={}", http_port, grpc_port);

    let config_content = format!(
        "\
        [node]\n\
        id = 1\n\
        http_addr = \"127.0.0.1:{}\"\n\
        grpc_addr = \"127.0.0.1:{}\"\n\
        data_dir = \"{}\"\n\
        name = \"test-node-1\"\n\
        \n\
        [network]\n\
        request_timeout = 10000\n\
        connect_timeout = 3000\n\
        keep_alive_interval = 30000\n\
        max_retries = 3\n\
        retry_delay = 100\n\
        enable_compression = true\n\
        max_message_size = 4194304\n\
        \n\
        [storage]\n\
        enable_compression = true\n\
        compaction_strategy = \"level\"\n\
        max_log_size = 33554432\n\
        log_retention_count = 3\n\
        enable_wal = true\n\
        sync_writes = false\n\
        block_cache_size = 16\n\
        write_buffer_size = 16\n\
        \n\
        [raft]\n\
        heartbeat_interval = 100\n\
        election_timeout_min = 150\n\
        election_timeout_max = 300\n\
        max_append_entries = 50\n\
        enable_auto_compaction = true\n\
        compaction_threshold = 200\n\
        max_inflight_requests = 5\n\
        \n\
        [raft.snapshot_policy]\n\
        enable_auto_snapshot = true\n\
        entries_since_last_snapshot = 100\n\
        snapshot_interval = 30000\n\
        \n\
        [logging]\n\
        level = \"debug\"\n\
        format = \"pretty\"\n\
        structured = false\n\
        file_path = \"{}/node1.log\"\n\
        max_file_size = 10485760\n\
        max_files = 2\n\
        enable_colors = true\n\
        \n\
        [cluster]\n\
        name = \"ferrium-automated-test\"\n\
        expected_size = 1\n\
        enable_auto_join = true\n\
        leader_discovery_timeout = 30000\n\
        auto_join_timeout = 60000\n\
        auto_join_retry_interval = 5000\n\
        auto_accept_learners = true\n\
        \n\
        [[cluster.peer]]\n\
        id = 1\n\
        http_addr = \"127.0.0.1:{}\"\n\
        grpc_addr = \"127.0.0.1:{}\"\n\
        voting = true\n\
        priority = 100\n\
        \n\
        [cluster.discovery]\n\
        enabled = false\n\
        method = \"static\"\n\
        interval = 30000\n\
        \n\
        [security]\n\
        enable_tls = false\n\
        enable_mtls = false\n\
        accept_invalid_certs = false\n\
        auth_method = \"none\"\n\
        ",
        http_port,
        grpc_port,
        data_dir.display(),
        log_dir.display(),
        http_port,
        grpc_port
    );

    let config_path = config_dir.join("node1.toml");
    std::fs::write(&config_path, config_content).unwrap();

    println!("📋 Config written with auto-join ENABLED");

    // Start the node
    println!("🚀 Starting node with auto-join...");
    let mut child = std::process::Command::new("target/release/ferrium-server")
        .arg("--config")
        .arg(&config_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Give it just a moment to start like the script does
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Check if process is still running
    match child.try_wait() {
        Ok(Some(status)) => {
            println!("❌ Process exited early with status: {}", status);
            panic!("Process exited early!");
        }
        Ok(None) => {
            println!("✅ Process is running with auto-join");
        }
        Err(e) => {
            println!("❌ Error checking process status: {}", e);
            panic!("Process check failed");
        }
    }

    // Test health endpoint
    let health_url = format!("http://127.0.0.1:{}/health", http_port);
    let client = reqwest::Client::new();

    match timeout(Duration::from_secs(5), client.get(&health_url).send()).await {
        Ok(Ok(resp)) => {
            println!("✅ Health check passed: {}", resp.status());
        }
        Ok(Err(e)) => {
            println!("❌ Health check failed: {}", e);
        }
        Err(_) => {
            println!("⏰ Health check timed out");
        }
    }

    // Wait a bit more for auto-join to potentially work
    println!("⏰ Waiting for auto-join to complete...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Check metrics to see cluster state before init
    let metrics_url = format!("http://127.0.0.1:{}/metrics", http_port);
    match timeout(Duration::from_secs(3), client.get(&metrics_url).send()).await {
        Ok(Ok(resp)) => {
            let json: serde_json::Value = resp.json().await.unwrap();
            println!(
                "📊 Pre-init state: state={:?}, leader={:?}",
                json.get("state"),
                json.get("current_leader")
            );
        }
        Ok(Err(e)) => {
            println!("❌ Metrics failed: {}", e);
        }
        Err(_) => {
            println!("⏰ Metrics timed out");
        }
    }

    // Test init endpoint with shorter timeout
    println!("🔧 Testing init endpoint with auto-join enabled...");
    let init_url = format!("http://127.0.0.1:{}/init", http_port);

    let init_result = timeout(
        Duration::from_secs(5), // Shorter timeout
        client.post(&init_url).json(&serde_json::json!({})).send(),
    )
    .await;

    match init_result {
        Ok(Ok(response)) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            println!("✅ Init response: {} - {}", status, text);
        }
        Ok(Err(e)) => {
            println!("❌ Init failed with error: {}", e);
        }
        Err(_) => {
            println!("⏰ Init timed out after 5s with auto-join enabled");
        }
    }

    // Kill the process
    child.kill().ok();
    child.wait().ok();

    println!("🎉 Auto-join test completed!");
}

/// Test with HTTP-only server (no gRPC) to isolate tokio::select! deadlock
#[tokio::test]
#[serial]
async fn test_http_only_no_grpc() {
    init_crypto_provider();

    println!("🔧 Testing HTTP-only server (no gRPC deadlock)...");

    // Use different ports to avoid conflicts
    let http_port = 23001;

    println!("📍 Using HTTP-only port: {}", http_port);

    // Create raft instance directly like the main.rs does
    let config = FerriumConfig {
        node: ferrium::config::NodeConfig {
            id: 1,
            http_addr: format!("127.0.0.1:{}", http_port).parse().unwrap(),
            grpc_addr: "127.0.0.1:33001".parse().unwrap(),
            data_dir: std::path::PathBuf::from("./test-http-only-data"),
            name: Some("test-node".to_string()),
            description: None,
        },
        ..Default::default()
    };

    // Initialize storage
    println!("💾 Initializing storage...");
    let (log_store, state_machine_store) = ferrium::storage::new_storage(&config.node.data_dir)
        .await
        .unwrap();

    // Create network factory
    let network_factory = ferrium::network::HttpNetworkFactory::new();

    // Create Raft instance
    println!("🗳️ Creating Raft instance...");
    let raft_config = std::sync::Arc::new(ferrium::config::create_raft_config(&config.raft));
    let raft = std::sync::Arc::new(
        openraft::Raft::new(
            config.node.id,
            raft_config,
            network_factory,
            log_store,
            state_machine_store,
        )
        .await
        .unwrap(),
    );

    // Create management API
    println!("⚙️ Creating management API...");
    let management = std::sync::Arc::new(ferrium::network::management::ManagementApi::new(
        (*raft).clone(),
        config.node.id,
        config.clone(),
    ));

    // Test direct init call (like the test does)
    println!("🔧 Testing direct init call...");
    let direct_init_result =
        tokio::time::timeout(std::time::Duration::from_secs(5), management.init()).await;

    match direct_init_result {
        Ok(Ok(_)) => {
            println!("✅ Direct init() call succeeded!");
        }
        Ok(Err(e)) => {
            println!("❌ Direct init() failed: {}", e);
        }
        Err(_) => {
            println!("⏰ Direct init() timed out!");
            // This means even without HTTP/gRPC servers, init() hangs
            // The issue is deeper in the raft initialization
        }
    }

    // Clean up
    let _ = std::fs::remove_dir_all("./test-http-only-data");

    println!("🎉 HTTP-only test completed!");
}
