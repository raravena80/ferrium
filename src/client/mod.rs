use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::json;
use tracing::{debug, error};

use crate::tls::ClientTlsConfig;
use crate::{FerriumError, KvRequest, NodeId};

/// Client for interacting with a Ferrium cluster
pub struct FerriumClient {
    client: Client,
    nodes: Vec<String>,
    current_leader: Option<String>,
    use_tls: bool,
}

impl FerriumClient {
    /// Create a new client with a list of node addresses
    pub fn new(nodes: Vec<String>) -> Self {
        Self::with_tls_config(nodes, None)
    }

    /// Create a new client with TLS configuration
    pub fn with_tls_config(nodes: Vec<String>, tls_config: Option<Arc<ClientTlsConfig>>) -> Self {
        let mut client_builder = Client::builder().timeout(Duration::from_secs(30));

        let use_tls = tls_config.is_some();

        if let Some(tls_config) = tls_config {
            match tls_config.create_rustls_client_config() {
                Ok(rustls_config) => {
                    // Extract the ClientConfig from Arc for reqwest compatibility
                    let config =
                        Arc::try_unwrap(rustls_config).unwrap_or_else(|arc| (*arc).clone());
                    client_builder = client_builder.use_preconfigured_tls(config);
                }
                Err(e) => {
                    tracing::error!("Failed to configure TLS for FerriumClient: {}", e);
                    // Continue without TLS rather than panicking
                }
            }
        }

        let client = client_builder
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            nodes,
            current_leader: None,
            use_tls,
        }
    }

    /// Create a new client that accepts invalid certificates (for testing only)
    pub fn with_insecure_tls(nodes: Vec<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            nodes,
            current_leader: None,
            use_tls: true, // We're using HTTPS URLs, so mark as TLS
        }
    }

    /// Add a node address to the client
    pub fn add_node(&mut self, addr: String) {
        if !self.nodes.contains(&addr) {
            self.nodes.push(addr);
        }
    }

    /// Get the list of node addresses
    pub fn nodes(&self) -> &[String] {
        &self.nodes
    }

    /// Get the number of nodes
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Construct URL with the correct protocol (HTTP or HTTPS)
    fn make_url(&self, addr: &str, endpoint: &str) -> String {
        let protocol = if self.use_tls { "https" } else { "http" };
        format!(
            "{}://{}/{}",
            protocol,
            addr,
            endpoint.trim_start_matches('/')
        )
    }

    /// Get the current leader or try to find one
    async fn get_leader(&mut self) -> Result<String, FerriumError> {
        // If we have a cached leader, try it first
        if let Some(ref leader) = self.current_leader {
            if self.is_leader(leader).await.unwrap_or(false) {
                return Ok(leader.clone());
            }
        }

        // Helper function to extract host:port from full URL
        fn extract_addr(url: &str) -> &str {
            if let Some(addr) = url.strip_prefix("http://") {
                addr
            } else if let Some(addr) = url.strip_prefix("https://") {
                addr
            } else {
                url // Already just host:port
            }
        }

        // Try to find the leader by querying all nodes
        let mut leader_candidate = None;
        let mut known_leader_id = None;

        for full_url in &self.nodes {
            let addr = extract_addr(full_url);
            match self.get_metrics(addr).await {
                Ok(metrics) => {
                    // Check if this node is the leader
                    if matches!(
                        metrics.get("state").and_then(|v| v.as_str()),
                        Some("Leader")
                    ) {
                        self.current_leader = Some(full_url.clone());
                        return Ok(full_url.clone());
                    }

                    // Remember if this node knows who the leader is
                    if let Some(leader_id) = metrics.get("current_leader").and_then(|v| v.as_u64())
                    {
                        if leader_id != 0 {
                            known_leader_id = Some(leader_id);
                        }
                    }

                    // If node is a follower or candidate, it might become leader soon
                    if matches!(
                        metrics.get("state").and_then(|v| v.as_str()),
                        Some("Follower") | Some("Candidate")
                    ) && leader_candidate.is_none()
                    {
                        leader_candidate = Some(full_url.clone());
                    }
                }
                Err(e) => {
                    debug!("Failed to get metrics from {}: {}", addr, e);
                    continue;
                }
            }
        }

        // If we found a leader ID but no leader node, wait a bit for election to complete
        if known_leader_id.is_some() && leader_candidate.is_some() {
            // Give some time for leader election to complete
            tokio::time::sleep(Duration::from_millis(200)).await;

            // Try one more time to find the actual leader
            for full_url in &self.nodes {
                let addr = extract_addr(full_url);
                if let Ok(metrics) = self.get_metrics(addr).await {
                    if matches!(
                        metrics.get("state").and_then(|v| v.as_str()),
                        Some("Leader")
                    ) {
                        self.current_leader = Some(full_url.clone());
                        return Ok(full_url.clone());
                    }
                }
            }
        }

        Err(FerriumError::Network("No leader found".to_string()))
    }

    /// Check if a node is the current leader
    async fn is_leader(&self, addr: &str) -> Result<bool, FerriumError> {
        // Helper function to extract host:port from full URL
        fn extract_addr(url: &str) -> &str {
            if let Some(addr) = url.strip_prefix("http://") {
                addr
            } else if let Some(addr) = url.strip_prefix("https://") {
                addr
            } else {
                url // Already just host:port
            }
        }

        let host_port = extract_addr(addr);
        match self.get_metrics(host_port).await {
            Ok(metrics) => Ok(matches!(
                metrics.get("state").and_then(|v| v.as_str()),
                Some("Leader")
            )),
            Err(_) => Ok(false),
        }
    }

    /// Get cluster metrics from a specific node
    async fn get_metrics(&self, addr: &str) -> Result<serde_json::Value, FerriumError> {
        let url = self.make_url(addr, "metrics");
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| FerriumError::Network(format!("Failed to connect to {addr}: {e}")))?;

        if !response.status().is_success() {
            return Err(FerriumError::Network(format!(
                "Request to {} failed with status: {}",
                addr,
                response.status()
            )));
        }

        let metrics: serde_json::Value = response.json().await.map_err(|e| {
            FerriumError::Network(format!("Failed to parse metrics from {addr}: {e}"))
        })?;

        Ok(metrics)
    }

    /// Send a request to a specific endpoint
    async fn send_request(
        &self,
        addr: &str,
        endpoint: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, FerriumError> {
        // Helper function to extract host:port from full URL
        fn extract_addr(url: &str) -> &str {
            if let Some(addr) = url.strip_prefix("http://") {
                addr
            } else if let Some(addr) = url.strip_prefix("https://") {
                addr
            } else {
                url // Already just host:port
            }
        }

        let host_port = extract_addr(addr);
        let url = self.make_url(host_port, endpoint);
        debug!("Sending {} request to: {}", endpoint, url);

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                error!("Network error sending to {}: {}", url, e);
                FerriumError::Network(format!("Failed to connect to {addr}: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Request failed with status {}: {}", status, body);
            return Err(FerriumError::Network(format!(
                "Request failed with status {status}: {body}"
            )));
        }

        let result: serde_json::Value = response.json().await.map_err(|e| {
            error!("Failed to parse response from {}: {}", url, e);
            FerriumError::Network(format!("Failed to parse response: {e}"))
        })?;

        debug!("Request to {} completed successfully", url);
        Ok(result)
    }

    /// Set a key-value pair
    pub async fn set(&mut self, key: String, value: String) -> Result<(), FerriumError> {
        let leader = self.get_leader().await?;
        let request = KvRequest::Set {
            key: key.clone(),
            value: value.clone(),
        };

        debug!("Sending write request to {}: {:?}", leader, request);

        let result = self.send_request(&leader, "write", json!(request)).await?;

        debug!("Write response from {}: {:?}", leader, result);

        if result.get("error").is_some() {
            let error_msg = result.get("error").unwrap();
            error!("Write failed with error: {}", error_msg);
            return Err(FerriumError::Raft(format!("Write failed: {error_msg}")));
        }

        debug!("Write operation successful for key: {}", key);
        Ok(())
    }

    /// Get a value by key
    pub async fn get(&mut self, key: String) -> Result<Option<String>, FerriumError> {
        let leader = self.get_leader().await?;

        // Server expects: {"key": "keyname"}
        let request_payload = json!({"key": key});
        debug!("Sending read request to {}: {:?}", leader, request_payload);

        let result = self.send_request(&leader, "read", request_payload).await?;

        debug!("Read response from {}: {:?}", leader, result);

        if let Some(error) = result.get("error") {
            error!("Read failed with error: {}", error);
            return Err(FerriumError::Raft(format!("Read failed: {error}")));
        }

        Ok(result
            .get("value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    /// Delete a key
    pub async fn delete(&mut self, key: String) -> Result<(), FerriumError> {
        let leader = self.get_leader().await?;
        let request = KvRequest::Delete { key };

        let result = self.send_request(&leader, "write", json!(request)).await?;

        if result.get("error").is_some() {
            return Err(FerriumError::Raft(format!("Delete failed: {result}")));
        }

        Ok(())
    }

    /// Initialize a single-node cluster
    pub async fn init(&mut self) -> Result<(), FerriumError> {
        // Try to initialize on any node
        for addr in self.nodes.clone() {
            match self.send_request(&addr, "init", json!({})).await {
                Ok(result) => {
                    if result.get("error").is_none() {
                        self.current_leader = Some(addr);
                        return Ok(());
                    }
                }
                Err(e) => {
                    debug!("Failed to initialize on {}: {}", addr, e);
                    continue;
                }
            }
        }

        Err(FerriumError::Network(
            "Failed to initialize cluster".to_string(),
        ))
    }

    /// Add a learner node to the cluster
    pub async fn add_learner(&mut self, node_id: NodeId, addr: String) -> Result<(), FerriumError> {
        let leader = self.get_leader().await?;

        let result = self
            .send_request(&leader, "add-learner", json!([node_id, addr]))
            .await?;

        if result.get("error").is_some() {
            return Err(FerriumError::Raft(format!("Add learner failed: {result}")));
        }

        // Add the new node to our list
        self.add_node(addr);

        Ok(())
    }

    /// Change cluster membership
    pub async fn change_membership(&mut self, members: Vec<NodeId>) -> Result<(), FerriumError> {
        let leader = self.get_leader().await?;

        let result = self
            .send_request(&leader, "change-membership", json!(members))
            .await?;

        if result.get("error").is_some() {
            return Err(FerriumError::Raft(format!(
                "Change membership failed: {result}"
            )));
        }

        Ok(())
    }

    /// Get cluster metrics
    pub async fn metrics(&self) -> Result<serde_json::Value, FerriumError> {
        for addr in &self.nodes {
            match self.get_metrics(addr).await {
                Ok(metrics) => return Ok(metrics),
                Err(_) => continue,
            }
        }

        Err(FerriumError::Network("No nodes available".to_string()))
    }

    /// Wait for cluster to be ready (all nodes online)
    pub async fn wait_for_ready(&mut self, timeout: Duration) -> Result<(), FerriumError> {
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            match self.get_leader().await {
                Ok(_) => return Ok(()),
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }
        }

        Err(FerriumError::Network(
            "Timeout waiting for cluster to be ready".to_string(),
        ))
    }
}

// Client tests are in test.rs
#[cfg(test)]
mod test;
