use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::config::Node;
use crate::grpc::{
    AddLearnerRequest, AddLearnerResponse, ChangeMembershipRequest, ChangeMembershipResponse,
    HealthRequest, HealthResponse, InitializeRequest, InitializeResponse, LeaderRequest,
    LeaderResponse, ManagementServiceTrait, MembershipConfig, MetricsRequest, MetricsResponse,
    NodeInfo, ReplicationStatus,
};
use crate::network::management::ManagementApi;

pub struct ManagementServiceImpl {
    management: Arc<ManagementApi>,
}

impl ManagementServiceImpl {
    pub fn new(management: Arc<ManagementApi>) -> Self {
        Self { management }
    }
}

#[tonic::async_trait]
impl ManagementServiceTrait for ManagementServiceImpl {
    async fn initialize(
        &self,
        _request: Request<InitializeRequest>,
    ) -> Result<Response<InitializeResponse>, Status> {
        match self.management.init().await {
            Ok(_) => Ok(Response::new(InitializeResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(InitializeResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }

    async fn add_learner(
        &self,
        request: Request<AddLearnerRequest>,
    ) -> Result<Response<AddLearnerResponse>, Status> {
        let req = request.into_inner();

        let node = Node {
            rpc_addr: req.rpc_addr,
            api_addr: req.api_addr,
        };

        match self.management.add_learner(req.node_id, node).await {
            Ok(_) => Ok(Response::new(AddLearnerResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(AddLearnerResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }

    async fn change_membership(
        &self,
        request: Request<ChangeMembershipRequest>,
    ) -> Result<Response<ChangeMembershipResponse>, Status> {
        let req = request.into_inner();

        match self.management.change_membership(req.member_ids).await {
            Ok(_) => Ok(Response::new(ChangeMembershipResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(ChangeMembershipResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }

    async fn get_metrics(
        &self,
        _request: Request<MetricsRequest>,
    ) -> Result<Response<MetricsResponse>, Status> {
        let metrics = self.management.metrics().await;

        // Convert openraft metrics to protobuf format - simplified version
        let membership = MembershipConfig {
            log_index: 0,                            // TODO: Fix proper conversion
            voters: vec![],                          // TODO: Fix proper conversion
            learners: vec![],                        // TODO: Fix proper conversion
            nodes: std::collections::HashMap::new(), // TODO: Fix proper conversion
        };

        let replication = std::collections::HashMap::new(); // TODO: Fix replication mapping

        Ok(Response::new(MetricsResponse {
            state: format!("{:?}", metrics.state),
            current_leader: metrics.current_leader,
            term: 0,               // TODO: Fix term access
            last_log_index: 0,     // TODO: Fix index access
            last_applied_index: 0, // TODO: Fix index access
            membership: Some(membership),
            replication,
            error: String::new(),
        }))
    }

    async fn health_check(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: "healthy".to_string(),
            service: "ferrite".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }))
    }

    async fn get_leader(
        &self,
        _request: Request<LeaderRequest>,
    ) -> Result<Response<LeaderResponse>, Status> {
        let metrics = self.management.metrics().await;

        if let Some(leader_id) = metrics.current_leader {
            if let Some(leader_node) = metrics.membership_config.membership().get_node(&leader_id) {
                Ok(Response::new(LeaderResponse {
                    leader_id: Some(leader_id),
                    leader_info: Some(NodeInfo {
                        rpc_addr: leader_node.rpc_addr.clone(),
                        api_addr: leader_node.api_addr.clone(),
                    }),
                    error: String::new(),
                }))
            } else {
                Ok(Response::new(LeaderResponse {
                    leader_id: Some(leader_id),
                    leader_info: None,
                    error: "Leader node not found in membership".to_string(),
                }))
            }
        } else {
            Ok(Response::new(LeaderResponse {
                leader_id: None,
                leader_info: None,
                error: String::new(),
            }))
        }
    }
}
