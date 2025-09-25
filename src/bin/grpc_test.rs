use std::sync::Arc;

use tonic::transport::Server;
use tracing_subscriber::EnvFilter;

use ferrium::grpc::{
    services::{KvServiceImpl, ManagementServiceImpl},
    kv::kv_service_server::KvServiceServer,
    management::management_service_server::ManagementServiceServer,
};
use ferrium::{
    config::{create_raft_config, FerriumConfig, NodeId, RaftConfig},
    network::{management::ManagementApi, HttpNetworkFactory},
    storage::new_storage,
};
use openraft::Raft;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("ferrium=info".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting gRPC-only test server");

    // Initialize storage
    let (log_store, state_machine_store) = new_storage("./test_data")
        .await
        .map_err(|e| format!("Storage error: {e}"))?;

    // Initialize network
    let network_factory = HttpNetworkFactory::new();

    // Create Raft instance
    let raft_config = RaftConfig::default();
    let config = Arc::new(create_raft_config(&raft_config));
    let raft = Arc::new(
        Raft::new(
            1 as NodeId,
            config,
            network_factory,
            log_store,
            state_machine_store,
        )
        .await
        .map_err(|e| format!("Raft error: {e}"))?,
    );

    // Create management API
    let node_id: NodeId = 1;
    let config = FerriumConfig::default();
    let management = Arc::new(ManagementApi::new((*raft).clone(), node_id, config));

    // Create gRPC services
    let kv_service = KvServiceImpl::new(management.clone());
    let management_service = ManagementServiceImpl::new(management.clone());

    let addr = "127.0.0.1:9001".parse()?;

    tracing::info!("Starting gRPC server on {}", addr);

    Server::builder()
        .add_service(KvServiceServer::new(kv_service))
        .add_service(ManagementServiceServer::new(management_service))
        .serve(addr)
        .await?;

    Ok(())
}
