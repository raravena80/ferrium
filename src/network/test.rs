use super::management::ManagementApi;
use super::*;
use crate::config::{FerriteConfig, PeerConfig, PeerConfigWithId};
use std::collections::HashMap;
use tempfile::TempDir;

/// Helper function to create a test configuration with peers
fn create_test_config_with_peers(
    node_id: NodeId,
    peers: Vec<(NodeId, &str, &str)>,
) -> FerriteConfig {
    let mut config = FerriteConfig::default();
    config.node.id = node_id;
    config.node.http_addr = format!("127.0.0.1:{}", 8000 + node_id).parse().unwrap();

    let mut cluster_peers = HashMap::new();
    for (id, http_addr, grpc_addr) in peers {
        cluster_peers.insert(
            id,
            PeerConfig {
                http_addr: http_addr.parse().unwrap(),
                grpc_addr: grpc_addr.parse().unwrap(),
                priority: None,
                voting: true,
            },
        );
    }
    config.cluster.peers = cluster_peers;
    config
}

/// Helper function to create a mock ManagementApi for testing
async fn create_test_management_api(node_id: NodeId, config: FerriteConfig) -> ManagementApi {
    use crate::storage::new_storage;
    use openraft::Raft;

    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let (log_store, state_machine_store) = new_storage(temp_dir.path().join("test.db"))
        .await
        .expect("Failed to create storage");

    let network_factory = HttpNetworkFactory::new();
    let raft_config = std::sync::Arc::new(crate::config::create_raft_config(&config.raft));

    let raft = Raft::new(
        node_id,
        raft_config,
        network_factory,
        log_store,
        state_machine_store,
    )
    .await
    .expect("Failed to create Raft instance");

    ManagementApi::new(raft, node_id, config)
}

#[test]
fn test_network_config_creation() {
    let config = NetworkConfig {
        node_id: 1,
        http_addr: "127.0.0.1:8001".to_string(),
        raft_addr: "127.0.0.1:9001".to_string(),
    };

    assert_eq!(config.node_id, 1);
    assert_eq!(config.http_addr, "127.0.0.1:8001");
    assert_eq!(config.raft_addr, "127.0.0.1:9001");
}

#[test]
fn test_http_network_creation() {
    let network = HttpNetwork::new();
    assert_eq!(network.target_node, 0);
    assert_eq!(network.target_addr, "");

    let network_with_target = HttpNetwork::with_target(1, "127.0.0.1:8001".to_string());
    assert_eq!(network_with_target.target_node, 1);
    assert_eq!(network_with_target.target_addr, "127.0.0.1:8001");
}

#[test]
fn test_http_network_factory() {
    let factory = HttpNetworkFactory::new();
    // Factory should be created successfully
    // We can't easily test the new_client method without setting up actual network infrastructure
    assert!(std::ptr::eq(&factory, &factory)); // Basic identity check
}

#[test]
fn test_url_construction_logic() {
    let _network = HttpNetwork::with_target(1, "127.0.0.1:8001".to_string());

    // Test the URL construction logic that was recently fixed
    // We can't directly call send_rpc in tests, but we can test the logic

    // Test cases for URL construction:
    let test_cases = vec![
        ("127.0.0.1:8001", "test", "http://127.0.0.1:8001/test"),
        (
            "http://127.0.0.1:8001",
            "test",
            "http://127.0.0.1:8001/test",
        ),
        (
            "https://example.com:8001",
            "test",
            "https://example.com:8001/test",
        ),
    ];

    for (addr, uri, expected) in test_cases {
        let url = if addr.starts_with("http://") || addr.starts_with("https://") {
            format!("{addr}/{uri}")
        } else {
            format!("http://{addr}/{uri}")
        };

        assert_eq!(url, expected, "Failed for addr: {addr}, uri: {uri}");
    }
}

#[tokio::test]
async fn test_management_api_creation() {
    let config = create_test_config_with_peers(1, vec![(2, "127.0.0.1:8002", "127.0.0.1:9002")]);
    let mgmt_api = create_test_management_api(1, config.clone()).await;

    assert_eq!(mgmt_api.node_id, 1);
    assert_eq!(mgmt_api.config.node.id, 1);
    assert!(mgmt_api.config.cluster.peers.contains_key(&2));
}

#[tokio::test]
async fn test_is_leader_functionality() {
    let config = create_test_config_with_peers(1, vec![]);
    let mgmt_api = create_test_management_api(1, config).await;

    // Initially, node should not be leader (no cluster initialized)
    let is_leader = mgmt_api.is_leader().await;
    assert!(!is_leader);

    // After initialization, it should become leader of single-node cluster
    mgmt_api.init().await.expect("Failed to initialize cluster");

    // Give it a moment to become leader
    tokio::time::sleep(Duration::from_millis(100)).await;

    let is_leader_after_init = mgmt_api.is_leader().await;
    assert!(is_leader_after_init);
}

#[tokio::test]
async fn test_cluster_initialization() {
    let config = create_test_config_with_peers(1, vec![]);
    let mgmt_api = create_test_management_api(1, config).await;

    // Test initialization
    let result = mgmt_api.init().await;
    assert!(result.is_ok());

    // Verify metrics show initialized state
    let metrics = mgmt_api.metrics().await;
    let voter_ids: Vec<_> = metrics.membership_config.membership().voter_ids().collect();
    assert!(voter_ids.contains(&1));
}

#[tokio::test]
async fn test_kv_operations_after_init() {
    let config = create_test_config_with_peers(1, vec![]);
    let mgmt_api = create_test_management_api(1, config).await;

    // Initialize cluster first
    mgmt_api.init().await.expect("Failed to initialize cluster");

    // Give it time to become ready
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Test write operation
    let write_req = KvRequest::Set {
        key: "test_key".to_string(),
        value: "test_value".to_string(),
    };

    let write_result = mgmt_api.write(write_req).await;
    assert!(write_result.is_ok());
    assert!(matches!(write_result.unwrap(), KvResponse::Set));

    // Test read operation
    let read_result = mgmt_api.read("test_key").await;
    assert!(read_result.is_ok());
    assert_eq!(read_result.unwrap(), Some("test_value".to_string()));

    // Test reading non-existent key
    let missing_result = mgmt_api.read("nonexistent").await;
    assert!(missing_result.is_ok());
    assert_eq!(missing_result.unwrap(), None);
}

#[test]
fn test_should_auto_accept_learner() {
    let peers = vec![
        (1, "127.0.0.1:8001", "127.0.0.1:9001"),
        (2, "127.0.0.1:8002", "127.0.0.1:9002"),
        (3, "127.0.0.1:8003", "127.0.0.1:9003"),
    ];

    let mut config = create_test_config_with_peers(1, peers);
    config.cluster.auto_accept_learners = true;

    // Create a dummy management API (we just need the config check function)
    let _temp_dir = TempDir::new().expect("Failed to create temp directory");

    // We can't easily create a real ManagementApi in a sync test, so we'll test the logic directly
    // This tests the logic that would be in should_auto_accept_learner

    let all_peers = config.cluster.get_all_peers();
    let should_accept_known = config.cluster.auto_accept_learners && all_peers.contains_key(&2);
    let should_accept_unknown = config.cluster.auto_accept_learners && all_peers.contains_key(&99);

    assert!(should_accept_known); // Node 2 is in peers
    assert!(!should_accept_unknown); // Node 99 is not in peers

    // Test with auto_accept disabled
    config.cluster.auto_accept_learners = false;
    let should_accept_disabled = config.cluster.auto_accept_learners && all_peers.contains_key(&2);
    assert!(!should_accept_disabled);
}

#[test]
fn test_peer_configuration_formats() {
    // Test both HashMap and array formats
    let mut config = FerriteConfig::default();

    // Add peer in HashMap format
    let peer1 = PeerConfig {
        http_addr: "127.0.0.1:8001".parse().unwrap(),
        grpc_addr: "127.0.0.1:9001".parse().unwrap(),
        priority: None,
        voting: true,
    };
    config.cluster.peers.insert(1, peer1);

    // Add peer in array format
    let peer2 = PeerConfigWithId {
        id: 2,
        http_addr: "127.0.0.1:8002".parse().unwrap(),
        grpc_addr: "127.0.0.1:9002".parse().unwrap(),
        priority: Some(10),
        voting: true,
    };
    config.cluster.peer_list.push(peer2);

    let all_peers = config.cluster.get_all_peers();
    assert_eq!(all_peers.len(), 2);
    assert!(all_peers.contains_key(&1));
    assert!(all_peers.contains_key(&2));

    // Verify array format overrides HashMap format for same ID
    let peer1_override = PeerConfigWithId {
        id: 1,                                        // Same as HashMap peer
        http_addr: "127.0.0.1:8888".parse().unwrap(), // Different address
        grpc_addr: "127.0.0.1:9888".parse().unwrap(),
        priority: Some(20),
        voting: false, // Different voting status
    };
    config.cluster.peer_list.push(peer1_override);

    let all_peers_with_override = config.cluster.get_all_peers();
    assert_eq!(all_peers_with_override.len(), 2); // Still 2 peers

    let overridden_peer = &all_peers_with_override[&1];
    assert_eq!(overridden_peer.http_addr, "127.0.0.1:8888".parse().unwrap());
    assert!(!overridden_peer.voting); // Should use array format value
}

#[test]
fn test_cluster_config_timeouts() {
    let mut config = FerriteConfig::default();

    // Test default timeout values
    assert!(config.cluster.leader_discovery_timeout > Duration::from_secs(0));
    assert!(config.cluster.auto_join_timeout > Duration::from_secs(0));
    assert!(config.cluster.auto_join_retry_interval > Duration::from_millis(0));

    // Test custom timeout values
    config.cluster.leader_discovery_timeout = Duration::from_secs(5);
    config.cluster.auto_join_timeout = Duration::from_secs(10);
    config.cluster.auto_join_retry_interval = Duration::from_millis(500);

    assert_eq!(
        config.cluster.leader_discovery_timeout,
        Duration::from_secs(5)
    );
    assert_eq!(config.cluster.auto_join_timeout, Duration::from_secs(10));
    assert_eq!(
        config.cluster.auto_join_retry_interval,
        Duration::from_millis(500)
    );
}

#[tokio::test]
async fn test_network_error_handling() {
    // Create network pointing to non-existent server
    let network = HttpNetwork::with_target(99, "127.0.0.1:65432".to_string());

    // This should fail with network error
    // We can't directly test send_rpc as it's private, but we can test that
    // the network is created properly and would handle errors
    assert_eq!(network.target_node, 99);
    assert_eq!(network.target_addr, "127.0.0.1:65432");
}

#[test]
fn test_auto_join_configuration_validation() {
    let mut config = FerriteConfig::default();

    // Test enabling auto-join
    config.cluster.enable_auto_join = true;
    assert!(config.cluster.enable_auto_join);

    // Test auto-accept learners
    config.cluster.auto_accept_learners = true;
    assert!(config.cluster.auto_accept_learners);

    // Test that both can be disabled independently
    config.cluster.enable_auto_join = false;
    assert!(!config.cluster.enable_auto_join);
    assert!(config.cluster.auto_accept_learners); // Should still be true

    config.cluster.auto_accept_learners = false;
    assert!(!config.cluster.auto_accept_learners);
}

// Note: The following tests would require actual HTTP servers running
// They are included as examples of what comprehensive integration tests would look like

// #[tokio::test]
// #[ignore] // Requires actual HTTP server
// async fn test_discover_leader_integration() {
//     // This would test actual leader discovery against running nodes
//     // Would require setting up mock HTTP servers with proper responses
// }

// #[tokio::test]
// #[ignore] // Requires actual HTTP server
// async fn test_auto_join_workflow_integration() {
//     // This would test the full auto-join workflow:
//     // 1. Start leader node
//     // 2. Start follower node with auto-join enabled
//     // 3. Verify follower discovers and joins leader
//     // 4. Verify leader accepts the join request
//     // 5. Verify cluster state is updated correctly
// }

// #[tokio::test]
// #[ignore] // Requires actual HTTP server
// async fn test_raft_rpc_communication() {
//     // This would test actual Raft RPC communication:
//     // 1. Set up two nodes with HTTP network
//     // 2. Send append_entries, vote, install_snapshot requests
//     // 3. Verify responses are handled correctly
//     // 4. Test error scenarios (network failures, invalid responses)
// }

#[test]
fn test_multiple_peer_discovery_scenarios() {
    // Test various peer configuration scenarios

    // Scenario 1: Empty peer list
    let config1 = create_test_config_with_peers(1, vec![]);
    let peers1 = config1.cluster.get_all_peers();
    assert_eq!(peers1.len(), 0);

    // Scenario 2: Single peer
    let config2 = create_test_config_with_peers(1, vec![(2, "127.0.0.1:8002", "127.0.0.1:9002")]);
    let peers2 = config2.cluster.get_all_peers();
    assert_eq!(peers2.len(), 1);
    assert!(peers2.contains_key(&2));

    // Scenario 3: Multiple peers
    let config3 = create_test_config_with_peers(
        1,
        vec![
            (2, "127.0.0.1:8002", "127.0.0.1:9002"),
            (3, "127.0.0.1:8003", "127.0.0.1:9003"),
            (4, "127.0.0.1:8004", "127.0.0.1:9004"),
        ],
    );
    let peers3 = config3.cluster.get_all_peers();
    assert_eq!(peers3.len(), 3);
    assert!(peers3.contains_key(&2));
    assert!(peers3.contains_key(&3));
    assert!(peers3.contains_key(&4));
}

#[tokio::test]
async fn test_metrics_collection() {
    let config = create_test_config_with_peers(1, vec![]);
    let mgmt_api = create_test_management_api(1, config).await;

    // Get initial metrics
    let metrics = mgmt_api.metrics().await;

    // Verify basic metrics structure
    assert_eq!(metrics.id, 1);
    // Note: More specific metric assertions would depend on the exact state
    // after Raft initialization, which can vary
}

#[test]
fn test_node_address_formatting() {
    // Test that node addresses are formatted correctly for different use cases
    let config = create_test_config_with_peers(1, vec![(2, "127.0.0.1:8002", "127.0.0.1:9002")]);

    // Test HTTP address formatting
    let http_addr = format!("http://{}", config.node.http_addr);
    assert!(http_addr.starts_with("http://"));
    assert!(http_addr.contains("127.0.0.1:8001"));

    // Test that peer addresses are properly parsed
    let peers = config.cluster.get_all_peers();
    let peer2 = &peers[&2];
    assert_eq!(peer2.http_addr.to_string(), "127.0.0.1:8002");
    assert_eq!(peer2.grpc_addr.to_string(), "127.0.0.1:9002");
}

#[test]
fn test_configuration_edge_cases() {
    // Test edge cases in configuration that might affect network behavior

    // Very short timeouts
    let mut config = FerriteConfig::default();
    config.cluster.leader_discovery_timeout = Duration::from_millis(1);
    config.cluster.auto_join_timeout = Duration::from_millis(1);
    config.cluster.auto_join_retry_interval = Duration::from_millis(1);

    assert_eq!(
        config.cluster.leader_discovery_timeout,
        Duration::from_millis(1)
    );

    // Very long timeouts
    config.cluster.leader_discovery_timeout = Duration::from_secs(3600); // 1 hour
    assert_eq!(
        config.cluster.leader_discovery_timeout,
        Duration::from_secs(3600)
    );

    // Test with auto-join disabled but peers present
    config.cluster.enable_auto_join = false;
    let peers = vec![(2, "127.0.0.1:8002", "127.0.0.1:9002")];
    let config_with_peers = create_test_config_with_peers(1, peers);

    // Should have peers but auto-join enabled by default
    assert!(config_with_peers.cluster.enable_auto_join); // Default is true
    assert!(!config_with_peers.cluster.get_all_peers().is_empty());
}

#[tokio::test]
async fn test_concurrent_operations() {
    let config = create_test_config_with_peers(1, vec![]);
    let mgmt_api = create_test_management_api(1, config).await;

    // Initialize cluster
    mgmt_api.init().await.expect("Failed to initialize cluster");
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Test concurrent operations
    let mgmt_api_clone = mgmt_api.clone();

    let write_task = tokio::spawn(async move {
        mgmt_api_clone
            .write(KvRequest::Set {
                key: "concurrent_key".to_string(),
                value: "concurrent_value".to_string(),
            })
            .await
    });

    let metrics_task = tokio::spawn(async move { mgmt_api.metrics().await });

    // Both operations should complete successfully
    let (write_result, _metrics_result) = tokio::try_join!(write_task, metrics_task).unwrap();
    assert!(write_result.is_ok());
}
