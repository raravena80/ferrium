use std::time::Duration;

use openraft::{
    error::{NetworkError, RPCError, RaftError, RemoteError},
    network::{RPCOption, RaftNetwork, RaftNetworkFactory},
    raft::{
        AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest,
        InstallSnapshotResponse, VoteRequest, VoteResponse,
    },
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use crate::config::{KvRequest, KvResponse, Node, NodeId, TypeConfig};

/// Network configuration for nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub node_id: NodeId,
    pub http_addr: String,
    pub raft_addr: String,
}

/// HTTP-based network implementation for Raft
#[derive(Debug, Clone)]
pub struct HttpNetwork {
    client: Client,
    target_node: NodeId,
    target_addr: String,
}

impl Default for HttpNetwork {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpNetwork {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            target_node: 0, // Will be set by new_client
            target_addr: String::new(),
        }
    }

    pub fn with_target(target_node: NodeId, target_addr: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            target_node,
            target_addr,
        }
    }

    async fn send_rpc<Req, Resp>(
        &self,
        uri: &str,
        req: Req,
    ) -> Result<Resp, RPCError<NodeId, Node, RaftError<NodeId>>>
    where
        Req: Serialize + Send,
        Resp: for<'a> Deserialize<'a> + Send,
    {
        let url = if self.target_addr.starts_with("http://")
            || self.target_addr.starts_with("https://")
        {
            format!("{}/{}", self.target_addr, uri)
        } else {
            format!("http://{}/{}", self.target_addr, uri)
        };

        debug!("Sending RPC to {}: {}", self.target_node, url);

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| {
                error!("Network error sending to {}: {}", url, e);
                RPCError::Network(NetworkError::new(&std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    format!("Failed to connect to {}: {}", self.target_addr, e),
                )))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("HTTP error {}: {}", status, body);

            // Try to parse as a remote error first
            if let Ok(remote_error) =
                serde_json::from_str::<RemoteError<NodeId, Node, RaftError<NodeId>>>(&body)
            {
                return Err(RPCError::RemoteError(remote_error));
            }

            return Err(RPCError::Network(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("RPC failed with status {status}: {body}"),
            ))));
        }

        let resp = response.json().await.map_err(|e| {
            error!("Failed to parse response from {}: {}", url, e);
            RPCError::Network(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse response: {e}"),
            )))
        })?;

        Ok(resp)
    }
}

impl RaftNetwork<TypeConfig> for HttpNetwork {
    async fn append_entries(
        &mut self,
        req: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, Node, RaftError<NodeId>>> {
        self.send_rpc("raft/append-entries", req).await
    }

    async fn install_snapshot(
        &mut self,
        req: InstallSnapshotRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<
            NodeId,
            Node,
            openraft::error::RaftError<NodeId, openraft::error::InstallSnapshotError>,
        >,
    > {
        // For install_snapshot, we need to use a different send_rpc signature due to different error type
        let url = if self.target_addr.starts_with("http://")
            || self.target_addr.starts_with("https://")
        {
            format!("{}/raft/install-snapshot", self.target_addr)
        } else {
            format!("http://{}/raft/install-snapshot", self.target_addr)
        };

        debug!(
            "Sending InstallSnapshot RPC to {}: {}",
            self.target_node, url
        );

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| {
                error!("Network error sending to {}: {}", url, e);
                RPCError::Network(NetworkError::new(&std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    format!("Failed to connect to {}: {}", self.target_addr, e),
                )))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("HTTP error {}: {}", status, body);

            return Err(RPCError::Network(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("RPC failed with status {status}: {body}"),
            ))));
        }

        let resp = response.json().await.map_err(|e| {
            error!("Failed to parse response from {}: {}", url, e);
            RPCError::Network(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse response: {e}"),
            )))
        })?;

        Ok(resp)
    }

    async fn vote(
        &mut self,
        req: VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, Node, RaftError<NodeId>>> {
        self.send_rpc("raft/vote", req).await
    }
}

/// Factory for creating network instances
pub struct HttpNetworkFactory {
    // We don't need to store anything here anymore since each network instance is self-contained
}

impl Default for HttpNetworkFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpNetworkFactory {
    pub fn new() -> Self {
        Self {}
    }
}

impl RaftNetworkFactory<TypeConfig> for HttpNetworkFactory {
    type Network = HttpNetwork;

    async fn new_client(&mut self, target: NodeId, node: &Node) -> Self::Network {
        HttpNetwork::with_target(target, node.rpc_addr.clone())
    }
}

/// Management API for handling administrative requests
pub mod management {
    use super::*;
    use crate::config::{FerriumConfig, PeerConfig};
    use openraft::{Raft, RaftMetrics};
    use std::time::Duration;

    #[derive(Clone)]
    pub struct ManagementApi {
        pub raft: Raft<TypeConfig>,
        pub node_id: NodeId,
        pub config: FerriumConfig,
    }

    impl ManagementApi {
        pub fn new(raft: Raft<TypeConfig>, node_id: NodeId, config: FerriumConfig) -> Self {
            Self {
                raft,
                node_id,
                config,
            }
        }

        pub async fn init(
            &self,
        ) -> Result<
            (),
            openraft::error::RaftError<NodeId, openraft::error::InitializeError<NodeId, Node>>,
        > {
            use std::collections::BTreeMap;

            let mut nodes = BTreeMap::new();
            // Use the actual node's configured address for initialization
            nodes.insert(
                self.node_id,
                Node {
                    rpc_addr: format!("http://{}", self.config.node.http_addr),
                    api_addr: format!("http://{}", self.config.node.http_addr),
                },
            );

            self.raft.initialize(nodes).await
        }

        pub async fn add_learner(&self, node_id: NodeId, node: Node) -> Result<(), anyhow::Error> {
            self.raft
                .add_learner(node_id, node, true)
                .await
                .map_err(|e| anyhow::anyhow!("Add learner failed: {}", e))?;
            Ok(())
        }

        pub async fn change_membership(&self, members: Vec<NodeId>) -> Result<(), anyhow::Error> {
            let _response = self
                .raft
                .change_membership(members, false)
                .await
                .map_err(|e| anyhow::anyhow!("Change membership failed: {}", e))?;
            Ok(())
        }

        pub async fn metrics(&self) -> RaftMetrics<NodeId, Node> {
            self.raft.metrics().borrow().clone()
        }

        pub async fn write(&self, req: KvRequest) -> Result<KvResponse, anyhow::Error> {
            let response = self
                .raft
                .client_write(req)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to write: {}", e))?;
            Ok(response.data)
        }

        pub async fn read(&self, key: &str) -> Result<Option<String>, anyhow::Error> {
            self.raft
                .ensure_linearizable()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to ensure linearizability: {}", e))?;

            let req = KvRequest::Get {
                key: key.to_string(),
            };
            match self.raft.client_write(req).await {
                Ok(response) => {
                    if let KvResponse::Get { value } = response.data {
                        Ok(value)
                    } else {
                        Ok(None)
                    }
                }
                Err(e) => Err(anyhow::anyhow!("Failed to read key {}: {}", key, e)),
            }
        }

        pub async fn is_leader(&self) -> bool {
            match self.raft.current_leader().await {
                Some(leader_id) => leader_id == self.node_id,
                None => false,
            }
        }

        pub async fn leader(&self) -> Option<NodeId> {
            self.raft.current_leader().await
        }

        /// Discover the current leader from known peers
        pub async fn discover_leader(
            &self,
            peers: &std::collections::HashMap<NodeId, PeerConfig>,
            timeout: Duration,
        ) -> Result<Option<(NodeId, String)>, anyhow::Error> {
            use reqwest::Client;
            use std::time::Instant;

            let client = Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

            let start = Instant::now();

            while start.elapsed() < timeout {
                for (&peer_id, peer_config) in peers {
                    // Skip self
                    if peer_id == self.node_id {
                        continue;
                    }

                    let url = format!("http://{}/leader", peer_config.http_addr);

                    match client.get(&url).send().await {
                        Ok(response) if response.status().is_success() => {
                            match response.json::<serde_json::Value>().await {
                                Ok(json) => {
                                    if let Some(leader_id) = json["leader"].as_u64() {
                                        tracing::info!(
                                            "Discovered leader: Node {} via Node {}",
                                            leader_id,
                                            peer_id
                                        );

                                        // Find the leader's HTTP address
                                        if let Some(leader_config) = peers.get(&leader_id) {
                                            return Ok(Some((
                                                leader_id,
                                                format!("http://{}", leader_config.http_addr),
                                            )));
                                        } else if leader_id == self.node_id {
                                            // We are the leader
                                            return Ok(None);
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "Failed to parse leader response from {}: {}",
                                        url,
                                        e
                                    );
                                }
                            }
                        }
                        Ok(response) => {
                            tracing::debug!(
                                "Non-success response from {}: {}",
                                url,
                                response.status()
                            );
                        }
                        Err(e) => {
                            tracing::debug!("Failed to contact peer {}: {}", url, e);
                        }
                    }
                }

                // Brief pause before retrying
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            Err(anyhow::anyhow!("Failed to discover leader within timeout"))
        }

        /// Attempt to join an existing cluster as a learner
        pub async fn auto_join_cluster(
            &self,
            config: &FerriumConfig,
        ) -> Result<bool, anyhow::Error> {
            if !config.cluster.enable_auto_join {
                tracing::info!("Auto-join is disabled");
                return Ok(false);
            }

            let all_peers = config.cluster.get_all_peers();
            if all_peers.is_empty() {
                tracing::info!("No peers configured, skipping auto-join");
                return Ok(false);
            }

            tracing::info!("Starting auto-join process...");

            // Wait a bit for other nodes to potentially elect a leader
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Try to discover the leader
            match self
                .discover_leader(&all_peers, config.cluster.leader_discovery_timeout)
                .await
            {
                Ok(Some((leader_id, leader_url))) => {
                    tracing::info!("Found leader: Node {} at {}", leader_id, leader_url);

                    // Request to join as learner
                    let join_result = self.request_join_as_learner(&leader_url, config).await;

                    match join_result {
                        Ok(()) => {
                            tracing::info!("Successfully joined cluster as learner");
                            Ok(true)
                        }
                        Err(e) => {
                            tracing::warn!("Failed to join as learner: {}", e);
                            Ok(false)
                        }
                    }
                }
                Ok(None) => {
                    tracing::info!("We appear to be the leader, no need to join");
                    Ok(false)
                }
                Err(e) => {
                    tracing::info!("No leader found during discovery: {}", e);
                    Ok(false)
                }
            }
        }

        /// Request to join as a learner to the specified leader
        async fn request_join_as_learner(
            &self,
            leader_url: &str,
            config: &FerriumConfig,
        ) -> Result<(), anyhow::Error> {
            use reqwest::Client;
            use serde_json::json;

            let client = Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

            let request_body = json!({
                "node_id": self.node_id,
                "rpc_addr": format!("http://{}", config.node.http_addr),
                "api_addr": format!("http://{}", config.node.http_addr)
            });

            let url = format!("{leader_url}/add-learner");

            let start = std::time::Instant::now();
            while start.elapsed() < config.cluster.auto_join_timeout {
                match client.post(&url).json(&request_body).send().await {
                    Ok(response) if response.status().is_success() => {
                        tracing::info!("Join request accepted by leader");
                        return Ok(());
                    }
                    Ok(response) => {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        tracing::warn!("Join request rejected: {} - {}", status, body);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to send join request: {}", e);
                    }
                }

                tokio::time::sleep(config.cluster.auto_join_retry_interval).await;
            }

            Err(anyhow::anyhow!("Auto-join timed out"))
        }

        /// Check if we should auto-accept this learner request
        pub fn should_auto_accept_learner(&self, node_id: NodeId, config: &FerriumConfig) -> bool {
            // Only auto-accept if enabled and the node is in our known peers
            let all_peers = config.cluster.get_all_peers();
            config.cluster.auto_accept_learners && all_peers.contains_key(&node_id)
        }
    }
}

pub mod api {
    use super::management::ManagementApi;
    use super::*;
    use crate::config::FerriumConfig;
    use actix_web::{web, HttpResponse, Result};
    use openraft::Raft;
    use serde_json::json;

    pub async fn append_entries(
        req: web::Json<AppendEntriesRequest<TypeConfig>>,
        raft: web::Data<Raft<TypeConfig>>,
    ) -> Result<HttpResponse> {
        match raft.append_entries(req.into_inner()).await {
            Ok(response) => Ok(HttpResponse::Ok().json(response)),
            Err(e) => Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()}))),
        }
    }

    pub async fn install_snapshot(
        req: web::Json<InstallSnapshotRequest<TypeConfig>>,
        raft: web::Data<Raft<TypeConfig>>,
    ) -> Result<HttpResponse> {
        match raft.install_snapshot(req.into_inner()).await {
            Ok(response) => Ok(HttpResponse::Ok().json(response)),
            Err(e) => Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()}))),
        }
    }

    pub async fn vote(
        req: web::Json<VoteRequest<NodeId>>,
        raft: web::Data<Raft<TypeConfig>>,
    ) -> Result<HttpResponse> {
        match raft.vote(req.into_inner()).await {
            Ok(response) => Ok(HttpResponse::Ok().json(response)),
            Err(e) => Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()}))),
        }
    }

    // Management endpoints
    pub async fn init(mgmt: web::Data<ManagementApi>) -> Result<HttpResponse> {
        match mgmt.init().await {
            Ok(_) => Ok(HttpResponse::Ok().json(json!({"status": "initialized"}))),
            Err(e) => Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()}))),
        }
    }

    pub async fn add_learner(
        req: web::Json<serde_json::Value>,
        mgmt: web::Data<ManagementApi>,
        config: web::Data<FerriumConfig>,
    ) -> Result<HttpResponse> {
        let node_id = req["node_id"].as_u64().unwrap() as NodeId;
        let rpc_addr = req["rpc_addr"].as_str().unwrap().to_string();
        let api_addr = req["api_addr"].as_str().unwrap().to_string();

        // Check if this is an auto-join request and if we should accept it
        let should_accept = mgmt.should_auto_accept_learner(node_id, &config);

        if !should_accept {
            // For now, we'll accept all requests, but log when auto-accept would reject
            tracing::warn!(
                "Learner join request from Node {} would be rejected by auto-accept policy",
                node_id
            );
        }

        let node = Node { rpc_addr, api_addr };

        match mgmt.add_learner(node_id, node).await {
            Ok(_) => {
                tracing::info!("Added Node {} as learner", node_id);
                Ok(HttpResponse::Ok().json(json!({"status": "learner added"})))
            }
            Err(e) => {
                tracing::warn!("Failed to add Node {} as learner: {}", node_id, e);
                Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()})))
            }
        }
    }

    pub async fn change_membership(
        req: web::Json<Vec<NodeId>>,
        mgmt: web::Data<ManagementApi>,
    ) -> Result<HttpResponse> {
        match mgmt.change_membership(req.into_inner()).await {
            Ok(_) => Ok(HttpResponse::Ok().json(json!({"status": "membership changed"}))),
            Err(e) => Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()}))),
        }
    }

    pub async fn write(
        req: web::Json<KvRequest>,
        mgmt: web::Data<ManagementApi>,
    ) -> Result<HttpResponse> {
        match mgmt.write(req.into_inner()).await {
            Ok(response) => Ok(HttpResponse::Ok().json(response)),
            Err(e) => Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()}))),
        }
    }

    pub async fn read(
        req: web::Json<serde_json::Value>,
        mgmt: web::Data<ManagementApi>,
    ) -> Result<HttpResponse> {
        let key = req["key"].as_str().unwrap();
        match mgmt.read(key).await {
            Ok(value) => Ok(HttpResponse::Ok().json(json!({"key": key, "value": value}))),
            Err(e) => Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()}))),
        }
    }

    pub async fn metrics(mgmt: web::Data<ManagementApi>) -> Result<HttpResponse> {
        let metrics = mgmt.metrics().await;
        Ok(HttpResponse::Ok().json(metrics))
    }

    pub async fn health() -> Result<HttpResponse> {
        Ok(HttpResponse::Ok().json(json!({
            "status": "healthy",
            "service": "ferrium",
            "version": env!("CARGO_PKG_VERSION")
        })))
    }

    pub async fn is_leader(mgmt: web::Data<ManagementApi>) -> Result<HttpResponse> {
        let is_leader = mgmt.is_leader().await;
        Ok(HttpResponse::Ok().json(json!({"is_leader": is_leader})))
    }

    pub async fn leader(mgmt: web::Data<ManagementApi>) -> Result<HttpResponse> {
        let leader = mgmt.leader().await;
        Ok(HttpResponse::Ok().json(json!({"leader": leader})))
    }
}

// Network tests are in test.rs
#[cfg(test)]
mod test;
