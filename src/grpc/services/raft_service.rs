use std::sync::Arc;

use openraft::Raft;
use tonic::{Request, Response, Status};

use crate::config::{NodeId, TypeConfig};
use crate::grpc::raft::{
    // Import the oneof variants
    append_entries_response::Result as AppendEntriesResult,
    raft_service_server::RaftService,
    AppendEntriesRequest as GrpcAppendEntriesRequest,
    AppendEntriesResponse as GrpcAppendEntriesResponse,
    AppendEntriesSuccess,
    InstallSnapshotRequest as GrpcInstallSnapshotRequest,
    InstallSnapshotResponse as GrpcInstallSnapshotResponse,
    VoteRequest as GrpcVoteRequest,
    VoteResponse as GrpcVoteResponse,
};

pub struct RaftServiceImpl {
    raft: Arc<Raft<TypeConfig>>,
}

impl RaftServiceImpl {
    pub fn new(raft: Arc<Raft<TypeConfig>>) -> Self {
        Self { raft }
    }
}

#[tonic::async_trait]
impl RaftService for RaftServiceImpl {
    async fn append_entries(
        &self,
        request: Request<GrpcAppendEntriesRequest>,
    ) -> Result<Response<GrpcAppendEntriesResponse>, Status> {
        let grpc_req = request.into_inner();

        // Convert gRPC request to openraft request
        let openraft_req = match convert_grpc_to_openraft_append_entries(grpc_req) {
            Ok(req) => req,
            Err(e) => return Err(Status::invalid_argument(format!("Invalid request: {e}"))),
        };

        // Call openraft
        match self.raft.append_entries(openraft_req).await {
            Ok(openraft_resp) => {
                let grpc_resp = convert_openraft_to_grpc_append_entries_response(openraft_resp);
                Ok(Response::new(grpc_resp))
            }
            Err(e) => Err(Status::internal(format!("Raft error: {e}"))),
        }
    }

    async fn vote(
        &self,
        request: Request<GrpcVoteRequest>,
    ) -> Result<Response<GrpcVoteResponse>, Status> {
        let grpc_req = request.into_inner();

        // Convert gRPC request to openraft request
        let openraft_req = match convert_grpc_to_openraft_vote(grpc_req) {
            Ok(req) => req,
            Err(e) => return Err(Status::invalid_argument(format!("Invalid request: {e}"))),
        };

        // Call openraft
        match self.raft.vote(openraft_req).await {
            Ok(openraft_resp) => {
                let grpc_resp = convert_openraft_to_grpc_vote_response(openraft_resp);
                Ok(Response::new(grpc_resp))
            }
            Err(e) => Err(Status::internal(format!("Raft error: {e}"))),
        }
    }

    async fn install_snapshot(
        &self,
        request: Request<GrpcInstallSnapshotRequest>,
    ) -> Result<Response<GrpcInstallSnapshotResponse>, Status> {
        let grpc_req = request.into_inner();

        // Convert gRPC request to openraft request
        let openraft_req = match convert_grpc_to_openraft_install_snapshot(grpc_req) {
            Ok(req) => req,
            Err(e) => return Err(Status::invalid_argument(format!("Invalid request: {e}"))),
        };

        // Call openraft
        match self.raft.install_snapshot(openraft_req).await {
            Ok(openraft_resp) => {
                let grpc_resp = convert_openraft_to_grpc_install_snapshot_response(openraft_resp);
                Ok(Response::new(grpc_resp))
            }
            Err(e) => Err(Status::internal(format!("Raft error: {e}"))),
        }
    }
}

// Conversion functions between gRPC and openraft types
// These are simplified implementations - in practice, you'd need complete conversions

fn convert_grpc_to_openraft_append_entries(
    _grpc_req: GrpcAppendEntriesRequest,
) -> Result<openraft::raft::AppendEntriesRequest<TypeConfig>, String> {
    // This is a complex conversion that would need to handle:
    // - Vote conversion
    // - LogId conversion
    // - Entry conversion
    // For now, return a placeholder error
    Err("AppendEntries conversion not yet implemented".to_string())
}

fn convert_openraft_to_grpc_append_entries_response(
    _openraft_resp: openraft::raft::AppendEntriesResponse<NodeId>,
) -> GrpcAppendEntriesResponse {
    // Placeholder implementation
    GrpcAppendEntriesResponse {
        result: Some(AppendEntriesResult::Success(AppendEntriesSuccess {})),
    }
}

fn convert_grpc_to_openraft_vote(
    _grpc_req: GrpcVoteRequest,
) -> Result<openraft::raft::VoteRequest<NodeId>, String> {
    // This would convert the gRPC vote request to openraft format
    Err("Vote conversion not yet implemented".to_string())
}

fn convert_openraft_to_grpc_vote_response(
    _openraft_resp: openraft::raft::VoteResponse<NodeId>,
) -> GrpcVoteResponse {
    // Placeholder implementation
    GrpcVoteResponse {
        vote: None,
        vote_granted: false,
        last_log_id: None,
    }
}

fn convert_grpc_to_openraft_install_snapshot(
    _grpc_req: GrpcInstallSnapshotRequest,
) -> Result<openraft::raft::InstallSnapshotRequest<TypeConfig>, String> {
    Err("InstallSnapshot conversion not yet implemented".to_string())
}

fn convert_openraft_to_grpc_install_snapshot_response(
    _openraft_resp: openraft::raft::InstallSnapshotResponse<NodeId>,
) -> GrpcInstallSnapshotResponse {
    // Placeholder implementation
    GrpcInstallSnapshotResponse { vote: None }
}
