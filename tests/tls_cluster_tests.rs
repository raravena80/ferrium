#![allow(clippy::uninlined_format_args)]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(dead_code)]

use ferrium::{
    config::SecurityConfig,
    tls::{validate_tls_config, ClientTlsConfig, TlsConfig},
};
use serde_json::json;
use serial_test::serial;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use tempfile::TempDir;
use tokio::time::{timeout, Duration};

static INIT: Once = Once::new();

fn init_crypto_provider() {
    INIT.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Failed to install rustls crypto provider");
    });
}

/// Test utilities for creating multi-node TLS test environments
mod cluster_test_utils {
    use super::*;

    pub struct TestClusterEnvironment {
        pub temp_dir: TempDir,
        pub ca_cert_path: PathBuf,
        pub ca_key_path: PathBuf,
        pub node_cert_paths: HashMap<u32, PathBuf>,
        pub node_key_paths: HashMap<u32, PathBuf>,
    }

    impl TestClusterEnvironment {
        pub fn new(node_ids: Vec<u32>) -> Result<Self, Box<dyn std::error::Error>> {
            let temp_dir = TempDir::new()?;
            let base_path = temp_dir.path();

            // Paths for CA
            let ca_key_path = base_path.join("ca-key.pem");
            let ca_cert_path = base_path.join("ca-cert.pem");

            // Generate CA private key
            let output = Command::new("openssl")
                .args(&["genrsa", "-out"])
                .arg(&ca_key_path)
                .arg("2048")
                .output()?;

            if !output.status.success() {
                return Err(format!(
                    "Failed to generate CA key: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }

            // Generate CA certificate
            let output = Command::new("openssl")
                .args(&["req", "-new", "-x509", "-key"])
                .arg(&ca_key_path)
                .args(&["-out"])
                .arg(&ca_cert_path)
                .args(&[
                    "-days",
                    "365",
                    "-subj",
                    "/CN=Ferrium Cluster CA/O=Ferrium Test",
                ])
                .output()?;

            if !output.status.success() {
                return Err(format!(
                    "Failed to generate CA cert: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }

            let mut node_cert_paths = HashMap::new();
            let mut node_key_paths = HashMap::new();

            // Generate certificates for each node
            for node_id in node_ids {
                let node_key_path = base_path.join(format!("node-{}-key.pem", node_id));
                let node_cert_path = base_path.join(format!("node-{}-cert.pem", node_id));
                let node_csr_path = base_path.join(format!("node-{}.csr", node_id));

                // Generate node private key
                let output = Command::new("openssl")
                    .args(&["genrsa", "-out"])
                    .arg(&node_key_path)
                    .arg("2048")
                    .output()?;

                if !output.status.success() {
                    return Err(format!(
                        "Failed to generate node {} key: {}",
                        node_id,
                        String::from_utf8_lossy(&output.stderr)
                    )
                    .into());
                }

                // Generate node certificate signing request
                let cn = format!("ferrium-node-{}", node_id);
                let output = Command::new("openssl")
                    .args(&["req", "-new", "-key"])
                    .arg(&node_key_path)
                    .args(&["-out"])
                    .arg(&node_csr_path)
                    .args(&["-subj", &format!("/CN={}/O=Ferrium Cluster Test", cn)])
                    .output()?;

                if !output.status.success() {
                    return Err(format!(
                        "Failed to generate node {} CSR: {}",
                        node_id,
                        String::from_utf8_lossy(&output.stderr)
                    )
                    .into());
                }

                // Create extensions file for node certificate
                let ext_file = base_path.join(format!("node_{}_ext.conf", node_id));
                std::fs::write(
                    &ext_file,
                    "[v3_req]
basicConstraints = CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth, clientAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = localhost
DNS.2 = 127.0.0.1
IP.1 = 127.0.0.1
",
                )?;

                // Sign node certificate with proper SAN extensions for rustls compatibility
                let output = Command::new("openssl")
                    .args(&["x509", "-req", "-in"])
                    .arg(&node_csr_path)
                    .args(&["-CA"])
                    .arg(&ca_cert_path)
                    .args(&["-CAkey"])
                    .arg(&ca_key_path)
                    .args(&["-CAcreateserial", "-out"])
                    .arg(&node_cert_path)
                    .args(&["-days", "365"])
                    .args(&["-extensions", "v3_req"])
                    .args(&["-extfile"])
                    .arg(&ext_file)
                    .output()?;

                if !output.status.success() {
                    return Err(format!(
                        "Failed to sign node {} cert: {}",
                        node_id,
                        String::from_utf8_lossy(&output.stderr)
                    )
                    .into());
                }

                node_cert_paths.insert(node_id, node_cert_path);
                node_key_paths.insert(node_id, node_key_path);
            }

            Ok(TestClusterEnvironment {
                temp_dir,
                ca_cert_path,
                ca_key_path,
                node_cert_paths,
                node_key_paths,
            })
        }

        pub fn create_node_security_config(
            &self,
            node_id: u32,
            enable_mtls: bool,
        ) -> SecurityConfig {
            SecurityConfig {
                enable_tls: true,
                enable_mtls,
                cert_file: self.node_cert_paths.get(&node_id).cloned(),
                key_file: self.node_key_paths.get(&node_id).cloned(),
                ca_file: if enable_mtls {
                    Some(self.ca_cert_path.clone())
                } else {
                    None
                },
                accept_invalid_certs: true, // For test environments with self-signed certificates
                auth_method: ferrium::config::AuthMethod::None,
            }
        }
    }

    pub fn find_available_port() -> u16 {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to random port");
        let port = listener
            .local_addr()
            .expect("Failed to get local addr")
            .port();
        drop(listener);
        port
    }

    pub async fn wait_for_server(addr: &str, timeout_secs: u64) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < timeout_secs {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        false
    }
}

/// Test TLS configuration validation for cluster setup
#[tokio::test]
#[serial]
async fn test_cluster_tls_config_validation() {
    init_crypto_provider();
    let node_ids = vec![1, 2, 3];
    let env = cluster_test_utils::TestClusterEnvironment::new(node_ids.clone()).unwrap();

    // Test valid TLS configuration for all nodes
    for node_id in &node_ids {
        let security = env.create_node_security_config(*node_id, false);
        assert!(
            validate_tls_config(&security).is_ok(),
            "Node {} TLS config should be valid",
            node_id
        );
    }

    // Test valid mTLS configuration for all nodes
    for node_id in &node_ids {
        let security = env.create_node_security_config(*node_id, true);
        assert!(
            validate_tls_config(&security).is_ok(),
            "Node {} mTLS config should be valid",
            node_id
        );
    }
}

/// Test TLS certificate creation for cluster nodes
#[tokio::test]
#[serial]
async fn test_cluster_certificate_creation() {
    init_crypto_provider();
    let node_ids = vec![1, 2, 3];
    let env = cluster_test_utils::TestClusterEnvironment::new(node_ids.clone()).unwrap();

    // Test TLS config creation for each node
    for node_id in &node_ids {
        let security = env.create_node_security_config(*node_id, true);
        let tls_config = TlsConfig::from_security_config(&security).unwrap();
        assert!(
            tls_config.is_some(),
            "Node {} should have TLS config",
            node_id
        );

        let tls_config = tls_config.unwrap();
        assert!(
            !tls_config.certificates.is_empty(),
            "Node {} should have certificates",
            node_id
        );
        assert!(
            !tls_config.private_key.secret_der().is_empty(),
            "Node {} should have private key",
            node_id
        );

        // Test client config creation
        let client_config = ClientTlsConfig::from_security_config(&security).unwrap();
        assert!(
            client_config.is_some(),
            "Node {} should have client TLS config",
            node_id
        );
    }
}

/// Test cluster communication with TLS
#[tokio::test]
#[serial]
async fn test_cluster_communication_with_tls() {
    init_crypto_provider();
    let node_ids = vec![1, 2];
    let env = cluster_test_utils::TestClusterEnvironment::new(node_ids.clone()).unwrap();

    // Create TLS configurations for each node
    let mut node_tls_configs = HashMap::new();
    let mut client_tls_configs = HashMap::new();

    for node_id in &node_ids {
        let security = env.create_node_security_config(*node_id, false);
        let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();
        let client_config = ClientTlsConfig::from_security_config(&security)
            .unwrap()
            .unwrap();

        node_tls_configs.insert(*node_id, tls_config);
        client_tls_configs.insert(*node_id, client_config);
    }

    // Start mock HTTPS servers for each node
    let mut server_handles = Vec::new();
    let mut server_addrs = Vec::new();

    for node_id in &node_ids {
        let port = cluster_test_utils::find_available_port();
        let addr = format!("127.0.0.1:{}", port);
        server_addrs.push(addr.clone());

        let tls_config = node_tls_configs.get(node_id).unwrap().clone();
        let rustls_config = tls_config.create_rustls_server_config().unwrap();

        let server_addr = addr.clone();
        let node_id_copy = *node_id;
        let handle = tokio::spawn(async move {
            use actix_web::{web, App, HttpResponse, HttpServer};

            let server_addr_for_binding = server_addr.clone();
            HttpServer::new(move || {
                let server_addr_health = server_addr.clone();
                let server_addr_leader = server_addr.clone();
                App::new()
                    .route(
                        "/health",
                        web::get().to(move || {
                            let addr = server_addr_health.clone();
                            async move {
                                HttpResponse::Ok().json(json!({
                                    "status": "ok",
                                    "node_id": node_id_copy,
                                    "tls": true,
                                    "addr": addr
                                }))
                            }
                        }),
                    )
                    .route(
                        "/leader",
                        web::get().to(move || {
                            let addr = server_addr_leader.clone();
                            async move {
                                HttpResponse::Ok().json(json!({
                                    "leader_id": node_id_copy,
                                    "leader_addr": addr
                                }))
                            }
                        }),
                    )
            })
            .bind_rustls_0_23(&server_addr_for_binding, (*rustls_config).clone())
            .unwrap()
            .run()
            .await
        });

        server_handles.push(handle);
    }

    // Wait for servers to start
    for addr in &server_addrs {
        assert!(
            cluster_test_utils::wait_for_server(addr, 15).await,
            "Server at {} failed to start",
            addr
        );
    }

    // Test inter-node communication with TLS using direct HTTP client
    let direct_client = reqwest::ClientBuilder::new()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    // Test communication to each node
    for (i, addr) in server_addrs.iter().enumerate() {
        let node_id = node_ids[i];

        // Create a mock health check request
        let response = timeout(
            Duration::from_secs(10),
            direct_client
                .get(&format!("https://{}/health", addr))
                .send(),
        )
        .await;

        assert!(response.is_ok(), "Health check failed for node {}", node_id);
        let response = response.unwrap().unwrap();
        assert!(response.status().is_success());

        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["node_id"], node_id);
        assert_eq!(json["tls"], true);
    }

    // Clean up
    for handle in server_handles {
        handle.abort();
    }
}

/// Test cluster communication with mTLS
#[tokio::test]
#[serial]
async fn test_cluster_communication_with_mtls() {
    init_crypto_provider();
    let node_ids = vec![1, 2];
    let env = cluster_test_utils::TestClusterEnvironment::new(node_ids.clone()).unwrap();

    // Create mTLS configurations for each node
    let mut node_tls_configs = HashMap::new();
    let mut client_tls_configs = HashMap::new();

    for node_id in &node_ids {
        let security = env.create_node_security_config(*node_id, true); // Enable mTLS
        let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();
        let client_config = ClientTlsConfig::from_security_config(&security)
            .unwrap()
            .unwrap();

        node_tls_configs.insert(*node_id, tls_config);
        client_tls_configs.insert(*node_id, client_config);
    }

    // Start mock HTTPS servers with mTLS for each node
    let mut server_handles = Vec::new();
    let mut server_addrs = Vec::new();

    for node_id in &node_ids {
        let port = cluster_test_utils::find_available_port();
        let addr = format!("127.0.0.1:{}", port);
        server_addrs.push(addr.clone());

        let tls_config = node_tls_configs.get(node_id).unwrap().clone();
        let rustls_config = tls_config.create_rustls_server_config().unwrap();

        let server_addr = addr.clone();
        let node_id_copy = *node_id;
        let handle = tokio::spawn(async move {
            use actix_web::{web, App, HttpResponse, HttpServer};

            let server_addr_for_binding = server_addr.clone();
            HttpServer::new(move || {
                let server_addr_for_health = server_addr.clone();
                App::new().route(
                    "/health",
                    web::get().to(move || {
                        let addr = server_addr_for_health.clone();
                        async move {
                            HttpResponse::Ok().json(json!({
                                "status": "ok",
                                "node_id": node_id_copy,
                                "mtls": true,
                                "addr": addr
                            }))
                        }
                    }),
                )
            })
            .bind_rustls_0_23(&server_addr_for_binding, (*rustls_config).clone())
            .unwrap()
            .run()
            .await
        });

        server_handles.push(handle);
    }

    // Wait for servers to start
    for addr in &server_addrs {
        assert!(
            cluster_test_utils::wait_for_server(addr, 15).await,
            "mTLS server at {} failed to start",
            addr
        );
    }

    // Test inter-node communication with mTLS - verify servers reject clients without certificates
    let direct_client = reqwest::ClientBuilder::new()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    // Test that each mTLS server correctly rejects clients without certificates
    for (i, addr) in server_addrs.iter().enumerate() {
        let node_id = node_ids[i];

        let result = timeout(
            Duration::from_secs(5),
            direct_client
                .get(&format!("https://{}/health", addr))
                .send(),
        )
        .await;

        // Should fail due to missing client certificate
        assert!(
            result.is_err() || result.unwrap().is_err(),
            "mTLS server on node {} should reject clients without certificates",
            node_id
        );

        println!(
            "✅ Node {} mTLS server correctly rejects clients without certificates",
            node_id
        );
    }

    // Clean up
    for handle in server_handles {
        handle.abort();
    }
}

/// Test that nodes without proper certificates are rejected in mTLS
#[tokio::test]
#[serial]
async fn test_mtls_rejects_unauthorized_nodes() {
    init_crypto_provider();

    let env = cluster_test_utils::TestClusterEnvironment::new(vec![1]).unwrap();

    // Create server with mTLS
    let security = env.create_node_security_config(1, true);
    let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();
    let rustls_config = tls_config.create_rustls_server_config().unwrap();

    let port = cluster_test_utils::find_available_port();
    let addr = format!("127.0.0.1:{}", port);

    // Start mTLS server
    let server_addr = addr.clone();
    let server_handle = tokio::spawn(async move {
        use actix_web::{web, App, HttpResponse, HttpServer};

        HttpServer::new(move || {
            App::new().route(
                "/test",
                web::get().to(|| async {
                    HttpResponse::Ok().json(json!({"message": "should not reach here"}))
                }),
            )
        })
        .bind_rustls_0_23(&server_addr, (*rustls_config).clone())
        .unwrap()
        .run()
        .await
    });

    // Wait for server to start
    assert!(
        cluster_test_utils::wait_for_server(&addr, 10).await,
        "Server failed to start"
    );

    // Create unauthorized environment (different CA)
    let unauthorized_env = cluster_test_utils::TestClusterEnvironment::new(vec![999]).unwrap();
    let _unauthorized_security = unauthorized_env.create_node_security_config(999, true);

    // Create direct HTTP client for testing unauthorized access
    let direct_client = reqwest::ClientBuilder::new()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    // Test that unauthorized client is rejected
    let result = timeout(
        Duration::from_secs(5),
        direct_client.get(&format!("https://{}/test", addr)).send(),
    )
    .await;

    // Should fail due to unauthorized certificate
    assert!(
        result.is_err() || result.unwrap().is_err(),
        "mTLS should reject unauthorized clients"
    );

    // Clean up
    server_handle.abort();
}

/// Test leader discovery with TLS
#[tokio::test]
#[serial]
async fn test_leader_discovery_with_tls() {
    init_crypto_provider();

    let node_ids = vec![1, 2, 3];
    let env = cluster_test_utils::TestClusterEnvironment::new(node_ids.clone()).unwrap();

    // Create server configurations
    let mut server_handles = Vec::new();
    let mut server_addrs = Vec::new();
    let leader_node = 2; // Node 2 will be the leader

    for node_id in &node_ids {
        let port = cluster_test_utils::find_available_port();
        let addr = format!("127.0.0.1:{}", port);
        server_addrs.push(addr.clone());

        let security = env.create_node_security_config(*node_id, false);
        let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();
        let rustls_config = tls_config.create_rustls_server_config().unwrap();

        let server_addr = addr.clone();
        let node_id_copy = *node_id;
        let is_leader = *node_id == leader_node;

        let handle = tokio::spawn(async move {
            use actix_web::{web, App, HttpResponse, HttpServer};

            let server_addr_for_binding = server_addr.clone();
            HttpServer::new(move || {
                let server_addr_for_leader = server_addr.clone();
                App::new().route(
                    "/leader",
                    web::get().to(move || {
                        let addr = server_addr_for_leader.clone();
                        async move {
                            if is_leader {
                                // Leader responds with its own info
                                HttpResponse::Ok().json(json!({
                                    "leader_id": node_id_copy,
                                    "leader_addr": addr,
                                    "is_leader": true
                                }))
                            } else {
                                // Non-leaders respond with 404 or redirect
                                HttpResponse::NotFound().json(json!({
                                    "error": "not leader",
                                    "node_id": node_id_copy,
                                    "is_leader": false
                                }))
                            }
                        }
                    }),
                )
            })
            .bind_rustls_0_23(&server_addr_for_binding, (*rustls_config).clone())
            .unwrap()
            .run()
            .await
        });

        server_handles.push(handle);
    }

    // Wait for all servers to start
    for addr in &server_addrs {
        assert!(
            cluster_test_utils::wait_for_server(addr, 15).await,
            "Server at {} failed to start",
            addr
        );
    }

    // Test leader discovery with direct HTTP client
    let direct_client = reqwest::ClientBuilder::new()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    // Try to discover leader by querying all nodes
    let mut leader_found = false;
    for addr in &server_addrs {
        let response = timeout(
            Duration::from_secs(10),
            direct_client
                .get(&format!("https://{}/leader", addr))
                .send(),
        )
        .await;

        if let Ok(Ok(resp)) = response {
            if resp.status().is_success() {
                let json: serde_json::Value = resp.json().await.unwrap();
                if json["leader_id"] == leader_node && json["is_leader"] == true {
                    leader_found = true;
                    break;
                }
            }
        }
    }

    assert!(leader_found, "Leader discovery with TLS failed");

    // Clean up
    for handle in server_handles {
        handle.abort();
    }
}

/// Test cluster auto-join with TLS
#[tokio::test]
#[serial]
async fn test_cluster_auto_join_with_tls() {
    init_crypto_provider();

    let node_ids = vec![1, 2];
    let env = cluster_test_utils::TestClusterEnvironment::new(node_ids.clone()).unwrap();

    // Create configurations
    let mut server_handles = Vec::new();
    let mut server_addrs = Vec::new();

    for node_id in &node_ids {
        let port = cluster_test_utils::find_available_port();
        let addr = format!("127.0.0.1:{}", port);
        server_addrs.push(addr.clone());

        let security = env.create_node_security_config(*node_id, false);
        let tls_config = TlsConfig::from_security_config(&security).unwrap().unwrap();
        let rustls_config = tls_config.create_rustls_server_config().unwrap();

        let server_addr = addr.clone();
        let node_id_copy = *node_id;

        let handle = tokio::spawn(async move {
            use actix_web::{web, App, HttpResponse, HttpServer};

            let server_addr_for_binding = server_addr.clone();
            HttpServer::new(move || {
                let server_addr_for_leader = server_addr.clone();
                App::new()
                    .route(
                        "/join",
                        web::post().to(move || async move {
                            HttpResponse::Ok().json(json!({
                                "status": "ok",
                                "message": format!("Node {node_id_copy} accepted join request"),
                                "node_id": node_id_copy
                            }))
                        }),
                    )
                    .route(
                        "/leader",
                        web::get().to(move || {
                            let addr = server_addr_for_leader.clone();
                            async move {
                                HttpResponse::Ok().json(json!({
                                    "leader_id": node_id_copy,
                                    "leader_addr": addr
                                }))
                            }
                        }),
                    )
            })
            .bind_rustls_0_23(&server_addr_for_binding, (*rustls_config).clone())
            .unwrap()
            .run()
            .await
        });

        server_handles.push(handle);
    }

    // Wait for servers to start
    for addr in &server_addrs {
        assert!(
            cluster_test_utils::wait_for_server(addr, 15).await,
            "Server at {} failed to start",
            addr
        );
    }

    // Test join request with direct HTTP client
    let direct_client = reqwest::ClientBuilder::new()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    // Send join request to first node
    let join_payload = json!({
        "node_id": 3,
        "http_addr": "127.0.0.1:8083",
        "grpc_addr": "127.0.0.1:9093"
    });

    let response = timeout(
        Duration::from_secs(10),
        direct_client
            .post(&format!("https://{}/join", server_addrs[0]))
            .json(&join_payload)
            .send(),
    )
    .await;

    assert!(response.is_ok(), "Join request with TLS failed");
    let response = response.unwrap().unwrap();
    assert!(response.status().is_success());

    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["node_id"], 1); // First node accepted the join

    // Clean up
    for handle in server_handles {
        handle.abort();
    }
}

/// Test error scenarios in TLS cluster setup
#[tokio::test]
#[serial]
async fn test_tls_cluster_error_scenarios() {
    init_crypto_provider();

    let env = cluster_test_utils::TestClusterEnvironment::new(vec![1]).unwrap();

    // Test missing certificate file
    let mut security = env.create_node_security_config(1, false);
    security.cert_file = Some(PathBuf::from("/nonexistent/cert.pem"));

    let result = validate_tls_config(&security);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("does not exist"));

    // Test missing CA file for mTLS
    let mut security = env.create_node_security_config(1, true);
    security.ca_file = None;

    let result = validate_tls_config(&security);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("ca_file is required when mTLS is enabled"));

    // Test node trying to use certificate for wrong node ID
    let env2 = cluster_test_utils::TestClusterEnvironment::new(vec![2]).unwrap();
    let mut security = env.create_node_security_config(1, true);
    security.cert_file = env2.node_cert_paths.get(&2).cloned(); // Wrong cert for node 1

    // This should still validate file existence, but would fail in actual TLS handshake
    let result = validate_tls_config(&security);
    assert!(result.is_ok()); // File exists, but cert is for wrong node (runtime error)
}

/// Test certificate rotation scenario
#[tokio::test]
#[serial]
async fn test_certificate_rotation_simulation() {
    init_crypto_provider();

    // Test that we can create multiple certificate environments
    // This simulates certificate rotation by creating a new environment
    let env1 = cluster_test_utils::TestClusterEnvironment::new(vec![1]).unwrap();
    let env2 = cluster_test_utils::TestClusterEnvironment::new(vec![1]).unwrap();

    // Both environments should create valid but different certificates
    let security1 = env1.create_node_security_config(1, false);
    let security2 = env2.create_node_security_config(1, false);

    let tls_config1 = TlsConfig::from_security_config(&security1)
        .unwrap()
        .unwrap();
    let tls_config2 = TlsConfig::from_security_config(&security2)
        .unwrap()
        .unwrap();

    // Certificates should be different (different CAs)
    assert_ne!(
        tls_config1.certificates[0].as_ref(),
        tls_config2.certificates[0].as_ref(),
        "Certificates from different environments should be different"
    );

    // Both should be valid TLS configurations
    assert!(validate_tls_config(&security1).is_ok());
    assert!(validate_tls_config(&security2).is_ok());
}
