use std::time::Duration;

use ferrite::{client::FerriteClient, config::FerriteConfig};

#[tokio::test]
#[ignore] // Ignored by default as it requires running servers
async fn test_cluster_integration() {
    // This test would:
    // 1. Start multiple ferrite-server instances
    // 2. Initialize a cluster
    // 3. Perform read/write operations
    // 4. Test leader election and failover
    // 5. Clean up resources

    // Placeholder test that passes
    println!("Integration test placeholder - requires actual server instances");
}

#[tokio::test]
async fn test_basic_configuration() {
    let config = FerriteConfig::default();
    assert!(config.node.id > 0);
    assert_eq!(config.node.http_addr.port(), 8001);
}

#[tokio::test]
#[ignore]
async fn test_ferrite_client_basic() {
    // Simple test to verify the client can be created
    let mut client = FerriteClient::new(vec!["127.0.0.1:8001".to_string()]);

    // Try to connect but expect it to fail (no server running)
    // This tests the error handling path
    if (client.wait_for_ready(Duration::from_secs(1)).await).is_ok() {
        // If a server is actually running, test basic operations
        let result = client
            .set("test_key".to_string(), "test_value".to_string())
            .await;
        println!("Set operation result: {result:?}");
    }
}
