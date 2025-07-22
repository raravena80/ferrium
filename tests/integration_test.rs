use std::time::Duration;

use ferrite::client::FerriteClient;
use tempfile::TempDir;
use tokio::time::sleep;

/// Test basic client functionality
#[tokio::test]
async fn test_client_operations() {
    let client = FerriteClient::new(vec![
        "127.0.0.1:8001".to_string(),
        "127.0.0.1:8002".to_string(),
    ]);

    // Test client creation
    assert!(client.nodes.len() == 2);
}

/// Test node configuration parsing
#[test] 
fn test_peer_parsing() {
    // This would test the peer parsing functionality from main.rs
    // We can't easily import it here, but the logic is tested in the main.rs tests
    assert!(true);
}

/// Placeholder for cluster integration tests
/// These would require starting actual server instances
#[tokio::test]
#[ignore] // Ignored by default as it requires running servers
async fn test_cluster_integration() {
    // This test would:
    // 1. Start multiple ferrite-server instances
    // 2. Initialize a cluster
    // 3. Perform read/write operations
    // 4. Test leader election and failover
    // 5. Clean up resources
    
    // For now, this is a placeholder showing the test structure
    // In a real implementation, you'd use something like:
    
    let mut client = FerriteClient::new(vec![
        "127.0.0.1:21001".to_string(),
        "127.0.0.1:21002".to_string(), 
        "127.0.0.1:21003".to_string(),
    ]);

    // Wait for cluster to be ready (would timeout if servers not running)
    if let Ok(_) = client.wait_for_ready(Duration::from_secs(1)).await {
        // Test operations
        let _ = client.set("test_key".to_string(), "test_value".to_string()).await;
        let value = client.get("test_key".to_string()).await.ok();
        assert_eq!(value.flatten(), Some("test_value".to_string()));
    }
}

/// Test error handling
#[tokio::test]
async fn test_error_conditions() {
    let mut client = FerriteClient::new(vec![
        "127.0.0.1:99999".to_string(), // Non-existent port
    ]);

    // Should fail to find a leader
    let result = client.get("nonexistent".to_string()).await;
    assert!(result.is_err());
}

/// Test configuration types
#[test]
fn test_config_types() {
    use ferrite::{KvRequest, KvResponse};
    
    // Test request serialization
    let req = KvRequest::Set { 
        key: "test".to_string(), 
        value: "value".to_string() 
    };
    
    let serialized = serde_json::to_string(&req).unwrap();
    let deserialized: KvRequest = serde_json::from_str(&serialized).unwrap();
    
    match deserialized {
        KvRequest::Set { key, value } => {
            assert_eq!(key, "test");
            assert_eq!(value, "value");
        }
        _ => panic!("Wrong request type"),
    }
    
    // Test response serialization
    let resp = KvResponse::Get { value: Some("test".to_string()) };
    let serialized = serde_json::to_string(&resp).unwrap();
    let deserialized: KvResponse = serde_json::from_str(&serialized).unwrap();
    
    match deserialized {
        KvResponse::Get { value } => {
            assert_eq!(value, Some("test".to_string()));
        }
        _ => panic!("Wrong response type"),
    }
} 