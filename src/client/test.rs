use super::*;

#[test]
fn test_client_creation() {
    let client = FerriteClient::new(vec![
        "127.0.0.1:8001".to_string(),
        "127.0.0.1:8002".to_string(),
    ]);

    assert_eq!(client.node_count(), 2);
    assert_eq!(client.nodes().len(), 2);
    assert!(client.current_leader.is_none());
}

#[test]
fn test_add_node() {
    let mut client = FerriteClient::new(vec!["127.0.0.1:8001".to_string()]);
    client.add_node("127.0.0.1:8002".to_string());

    assert_eq!(client.node_count(), 2);
    assert!(client.nodes().contains(&"127.0.0.1:8001".to_string()));
    assert!(client.nodes().contains(&"127.0.0.1:8002".to_string()));

    // Adding the same node again should not duplicate it
    client.add_node("127.0.0.1:8002".to_string());
    assert_eq!(client.node_count(), 2);
}

#[test]
fn test_nodes_getter() {
    let client = FerriteClient::new(vec![
        "127.0.0.1:8001".to_string(),
        "127.0.0.1:8002".to_string(),
    ]);

    let nodes = client.nodes();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0], "127.0.0.1:8001");
    assert_eq!(nodes[1], "127.0.0.1:8002");
}
