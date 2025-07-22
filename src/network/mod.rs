use std::sync::Arc;
use std::time::Duration;

use openraft::{
    error::{NetworkError, RPCError, RemoteError, RaftError},
    network::{RPCOption, RaftNetwork, RaftNetworkFactory}, 
    raft::{AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse, VoteRequest, VoteResponse},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, error};

use crate::config::{Node, NodeId, TypeConfig, KvRequest, KvResponse};

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
        let url = format!("http://{}/{}", self.target_addr, uri);

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
                    format!("Failed to connect to {}: {}", self.target_addr, e)
                )))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("HTTP error {}: {}", status, body);
            
            // Try to parse as a remote error first
            if let Ok(remote_error) = serde_json::from_str::<RemoteError<NodeId, Node, RaftError<NodeId>>>(&body) {
                return Err(RPCError::RemoteError(remote_error));
            }

            return Err(RPCError::Network(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("RPC failed with status {}: {}", status, body)
            ))));
        }

        let resp = response.json().await.map_err(|e| {
            error!("Failed to parse response from {}: {}", url, e);
            RPCError::Network(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse response: {}", e)
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
    ) -> Result<InstallSnapshotResponse<NodeId>, RPCError<NodeId, Node, openraft::error::RaftError<NodeId, openraft::error::InstallSnapshotError>>> {
        // For install_snapshot, we need to use a different send_rpc signature due to different error type
        let url = format!("http://{}/raft/install-snapshot", self.target_addr);

        debug!("Sending InstallSnapshot RPC to {}: {}", self.target_node, url);

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
                    format!("Failed to connect to {}: {}", self.target_addr, e)
                )))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("HTTP error {}: {}", status, body);
            
            return Err(RPCError::Network(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("RPC failed with status {}: {}", status, body)
            ))));
        }

        let resp = response.json().await.map_err(|e| {
            error!("Failed to parse response from {}: {}", url, e);
            RPCError::Network(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse response: {}", e)
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
    use openraft::{RaftMetrics, Raft};

    #[derive(Clone)]
    pub struct ManagementApi {
        pub raft: Raft<TypeConfig>,
        pub node_id: NodeId,
    }

    impl ManagementApi {
        pub fn new(raft: Raft<TypeConfig>, node_id: NodeId) -> Self {
            Self { raft, node_id }
        }

        pub async fn init(&self) -> Result<(), openraft::error::RaftError<NodeId, openraft::error::InitializeError<NodeId, Node>>> {
            use std::collections::BTreeMap;
            
            let mut nodes = BTreeMap::new();
            // For single-node initialization, use a default address
            // In practice, this should be configured based on the actual node's address
            nodes.insert(1, Node {
                rpc_addr: "127.0.0.1:8001".to_string(),
                api_addr: "127.0.0.1:8001".to_string(),
            });

            self.raft.initialize(nodes).await
        }

        pub async fn add_learner(
            &self,
            node_id: NodeId,
            node: Node,
        ) -> Result<(), anyhow::Error> {
            self.raft.add_learner(node_id, node, true).await
                .map_err(|e| anyhow::anyhow!("Add learner failed: {}", e))?;
            Ok(())
        }

        pub async fn change_membership(
            &self,
            members: Vec<NodeId>,
        ) -> Result<(), anyhow::Error> {
            let _response = self.raft.change_membership(members, false).await
                .map_err(|e| anyhow::anyhow!("Change membership failed: {}", e))?;
            Ok(())
        }

        pub async fn metrics(&self) -> RaftMetrics<NodeId, Node> {
            self.raft.metrics().borrow().clone()
        }

        pub async fn write(&self, req: KvRequest) -> Result<KvResponse, anyhow::Error> {
            let response = self.raft.client_write(req).await
                .map_err(|e| anyhow::anyhow!("Failed to write: {}", e))?;
            Ok(response.data)
        }

        pub async fn read(&self, key: &str) -> Result<Option<String>, anyhow::Error> {
            self.raft.ensure_linearizable().await
                .map_err(|e| anyhow::anyhow!("Failed to ensure linearizability: {}", e))?;
            
            let req = KvRequest::Get { key: key.to_string() };
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
    }
}

pub mod api {
    use super::management::ManagementApi;
    use super::*;
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
    ) -> Result<HttpResponse> {
        let node_id = req["node_id"].as_u64().unwrap() as NodeId;
        let rpc_addr = req["rpc_addr"].as_str().unwrap().to_string();
        let api_addr = req["api_addr"].as_str().unwrap().to_string();
        
        let node = Node { rpc_addr, api_addr };
        
        match mgmt.add_learner(node_id, node).await {
            Ok(_) => Ok(HttpResponse::Ok().json(json!({"status": "learner added"}))),
            Err(e) => Ok(HttpResponse::InternalServerError().json(json!({"error": e.to_string()}))),
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
            "service": "ferrite",
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