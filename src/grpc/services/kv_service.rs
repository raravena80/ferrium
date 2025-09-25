use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::config::KvRequest as InternalKvRequest;
use crate::grpc::kv::{
    kv_service_server::KvService, BatchSetRequest, BatchSetResponse, DeleteRequest, DeleteResponse,
    ExistsRequest, ExistsResponse, GetRequest, GetResponse, ListKeysRequest, ListKeysResponse,
    SetRequest, SetResponse,
};
use crate::network::management::ManagementApi;

pub struct KvServiceImpl {
    management: Arc<ManagementApi>,
}

impl KvServiceImpl {
    pub fn new(management: Arc<ManagementApi>) -> Self {
        Self { management }
    }
}

#[tonic::async_trait]
impl KvService for KvServiceImpl {
    async fn set(&self, request: Request<SetRequest>) -> Result<Response<SetResponse>, Status> {
        let req = request.into_inner();

        let kv_request = InternalKvRequest::Set {
            key: req.key,
            value: req.value,
        };

        match self.management.write(kv_request).await {
            Ok(_) => Ok(Response::new(SetResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(SetResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }

    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        let req = request.into_inner();

        match self.management.read(&req.key).await {
            Ok(Some(value)) => Ok(Response::new(GetResponse {
                key: req.key,
                value,
                found: true,
                error: String::new(),
            })),
            Ok(None) => Ok(Response::new(GetResponse {
                key: req.key,
                value: String::new(),
                found: false,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(GetResponse {
                key: req.key,
                value: String::new(),
                found: false,
                error: e.to_string(),
            })),
        }
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();

        // First check if key exists
        let existed = match self.management.read(&req.key).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => false, // Assume it doesn't exist if we can't read
        };

        let kv_request = InternalKvRequest::Delete { key: req.key };

        match self.management.write(kv_request).await {
            Ok(_) => Ok(Response::new(DeleteResponse {
                success: true,
                existed,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(DeleteResponse {
                success: false,
                existed,
                error: e.to_string(),
            })),
        }
    }

    async fn batch_set(
        &self,
        request: Request<BatchSetRequest>,
    ) -> Result<Response<BatchSetResponse>, Status> {
        let req = request.into_inner();
        let mut count = 0;

        for pair in req.pairs {
            let kv_request = InternalKvRequest::Set {
                key: pair.key,
                value: pair.value,
            };

            match self.management.write(kv_request).await {
                Ok(_) => count += 1,
                Err(e) => {
                    return Ok(Response::new(BatchSetResponse {
                        success: false,
                        count,
                        error: e.to_string(),
                    }));
                }
            }
        }

        Ok(Response::new(BatchSetResponse {
            success: true,
            count,
            error: String::new(),
        }))
    }

    async fn list_keys(
        &self,
        _request: Request<ListKeysRequest>,
    ) -> Result<Response<ListKeysResponse>, Status> {
        // This would require additional state machine support
        // For now, return not implemented
        Err(Status::unimplemented("ListKeys not yet implemented"))
    }

    async fn exists(
        &self,
        request: Request<ExistsRequest>,
    ) -> Result<Response<ExistsResponse>, Status> {
        let req = request.into_inner();

        match self.management.read(&req.key).await {
            Ok(Some(_)) => Ok(Response::new(ExistsResponse {
                exists: true,
                error: String::new(),
            })),
            Ok(None) => Ok(Response::new(ExistsResponse {
                exists: false,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(ExistsResponse {
                exists: false,
                error: e.to_string(),
            })),
        }
    }
}
