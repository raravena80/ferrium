use std::time::Duration;

use reqwest::Client;
use serde_json::json;
use tracing::{debug, error};

use crate::{FerriteError, KvRequest, NodeId};

/// Client for interacting with a Ferrite cluster
pub struct FerriteClient {
    client: Client,
    nodes: Vec<String>,
    current_leader: Option<String>,
}

impl FerriteClient {
    /// Create a new client with a list of node addresses
    pub fn new(nodes: Vec<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            nodes,
            current_leader: None,
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

    /// Get the current leader or try to find one
    async fn get_leader(&mut self) -> Result<String, FerriteError> {
        // If we have a cached leader, try it first
        if let Some(ref leader) = self.current_leader {
            if self.is_leader(leader).await.unwrap_or(false) {
                return Ok(leader.clone());
            }
        }

        // Try to find the leader by querying all nodes
        for addr in &self.nodes {
            match self.get_metrics(addr).await {
                Ok(metrics) => {
                    if matches!(
                        metrics.get("state").and_then(|v| v.as_str()),
                        Some("Leader")
                    ) {
                        self.current_leader = Some(addr.clone());
                        return Ok(addr.clone());
                    }

                    // If this node knows who the leader is
                    if let Some(_leader_id) = metrics.get("current_leader").and_then(|v| v.as_u64())
                    {
                        // Try to map leader ID to address (simplified approach)
                        for candidate_addr in &self.nodes {
                            if self.is_leader(candidate_addr).await.unwrap_or(false) {
                                self.current_leader = Some(candidate_addr.clone());
                                return Ok(candidate_addr.clone());
                            }
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to get metrics from {}: {}", addr, e);
                    continue;
                }
            }
        }

        Err(FerriteError::Network("No leader found".to_string()))
    }

    /// Check if a node is the current leader
    async fn is_leader(&self, addr: &str) -> Result<bool, FerriteError> {
        match self.get_metrics(addr).await {
            Ok(metrics) => Ok(matches!(
                metrics.get("state").and_then(|v| v.as_str()),
                Some("Leader")
            )),
            Err(_) => Ok(false),
        }
    }

    /// Get cluster metrics from a specific node
    async fn get_metrics(&self, addr: &str) -> Result<serde_json::Value, FerriteError> {
        let url = format!("http://{}/metrics", addr);
        let response =
            self.client.get(&url).send().await.map_err(|e| {
                FerriteError::Network(format!("Failed to connect to {}: {}", addr, e))
            })?;

        if !response.status().is_success() {
            return Err(FerriteError::Network(format!(
                "Request to {} failed with status: {}",
                addr,
                response.status()
            )));
        }

        let metrics: serde_json::Value = response.json().await.map_err(|e| {
            FerriteError::Network(format!("Failed to parse metrics from {}: {}", addr, e))
        })?;

        Ok(metrics)
    }

    /// Send a request to a specific endpoint
    async fn send_request(
        &self,
        addr: &str,
        endpoint: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, FerriteError> {
        let url = format!("http://{}/{}", addr, endpoint);
        debug!("Sending request to: {}", url);

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                error!("Network error sending to {}: {}", url, e);
                FerriteError::Network(format!("Failed to connect to {}: {}", addr, e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Request failed with status {}: {}", status, body);
            return Err(FerriteError::Network(format!(
                "Request failed with status {}: {}",
                status, body
            )));
        }

        let result: serde_json::Value = response.json().await.map_err(|e| {
            error!("Failed to parse response from {}: {}", url, e);
            FerriteError::Network(format!("Failed to parse response: {}", e))
        })?;

        debug!("Request to {} completed successfully", url);
        Ok(result)
    }

    /// Set a key-value pair
    pub async fn set(&mut self, key: String, value: String) -> Result<(), FerriteError> {
        let leader = self.get_leader().await?;
        let request = KvRequest::Set { key, value };

        let result = self.send_request(&leader, "write", json!(request)).await?;

        if result.get("error").is_some() {
            return Err(FerriteError::Raft(format!("Write failed: {}", result)));
        }

        Ok(())
    }

    /// Get a value by key
    pub async fn get(&mut self, key: String) -> Result<Option<String>, FerriteError> {
        let leader = self.get_leader().await?;

        let result = self.send_request(&leader, "read", json!(key)).await?;

        if let Some(error) = result.get("error") {
            return Err(FerriteError::Raft(format!("Read failed: {}", error)));
        }

        Ok(result
            .get("value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    /// Delete a key
    pub async fn delete(&mut self, key: String) -> Result<(), FerriteError> {
        let leader = self.get_leader().await?;
        let request = KvRequest::Delete { key };

        let result = self.send_request(&leader, "write", json!(request)).await?;

        if result.get("error").is_some() {
            return Err(FerriteError::Raft(format!("Delete failed: {}", result)));
        }

        Ok(())
    }

    /// Initialize a single-node cluster
    pub async fn init(&mut self) -> Result<(), FerriteError> {
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

        Err(FerriteError::Network(
            "Failed to initialize cluster".to_string(),
        ))
    }

    /// Add a learner node to the cluster
    pub async fn add_learner(&mut self, node_id: NodeId, addr: String) -> Result<(), FerriteError> {
        let leader = self.get_leader().await?;

        let result = self
            .send_request(&leader, "add-learner", json!([node_id, addr]))
            .await?;

        if result.get("error").is_some() {
            return Err(FerriteError::Raft(format!(
                "Add learner failed: {}",
                result
            )));
        }

        // Add the new node to our list
        self.add_node(addr);

        Ok(())
    }

    /// Change cluster membership
    pub async fn change_membership(&mut self, members: Vec<NodeId>) -> Result<(), FerriteError> {
        let leader = self.get_leader().await?;

        let result = self
            .send_request(&leader, "change-membership", json!(members))
            .await?;

        if result.get("error").is_some() {
            return Err(FerriteError::Raft(format!(
                "Change membership failed: {}",
                result
            )));
        }

        Ok(())
    }

    /// Get cluster metrics
    pub async fn metrics(&self) -> Result<serde_json::Value, FerriteError> {
        for addr in &self.nodes {
            match self.get_metrics(addr).await {
                Ok(metrics) => return Ok(metrics),
                Err(_) => continue,
            }
        }

        Err(FerriteError::Network("No nodes available".to_string()))
    }

    /// Wait for cluster to be ready (all nodes online)
    pub async fn wait_for_ready(&mut self, timeout: Duration) -> Result<(), FerriteError> {
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

        Err(FerriteError::Network(
            "Timeout waiting for cluster to be ready".to_string(),
        ))
    }
}

// Client tests are in test.rs
#[cfg(test)]
mod test;
