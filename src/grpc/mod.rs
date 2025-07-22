// Generated protobuf code
pub mod kv {
    include!("ferrite.kv.rs");
}

pub mod management {
    include!("ferrite.management.rs");
}

pub mod raft {
    include!("ferrite.raft.rs");
}

// Service implementations
pub mod services;

// Re-export commonly used types
pub use kv::{
    kv_service_server::{KvService as KvServiceTrait, KvServiceServer},
    *,
};

pub use management::{
    management_service_server::{
        ManagementService as ManagementServiceTrait, ManagementServiceServer,
    },
    *,
};

pub use raft::{
    raft_service_server::{RaftService as RaftServiceTrait, RaftServiceServer},
    *,
};
