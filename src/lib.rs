//! Ferrite - A distributed KV storage system built with openraft
//!
//! This library provides a distributed key-value store similar to etcd or Zookeeper,
//! implemented in Rust using the openraft consensus protocol library.

pub mod config;
pub mod network;
pub mod storage;
pub mod client;
pub mod grpc;

pub use config::{NodeId, TypeConfig, KvRequest, KvResponse, KvSnapshot, Node};
pub use network::{HttpNetwork, HttpNetworkFactory, NetworkConfig};
pub use storage::{LogStore, StateMachineStore, new_storage, FerriiteStorage};

#[derive(thiserror::Error, Debug)]
pub enum FerriteError {
    #[error("Storage error: {0}")]
    Storage(#[from] openraft::StorageError<NodeId>),
    
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Raft error: {0}")]
    Raft(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

// Test to understand openraft 0.9 API
#[cfg(test)]
mod test_api {
    use super::*;
    use openraft::*;
    
    // This will tell us the actual trait method signatures expected by openraft 0.9
    struct TestStorage;
    
    // Let's try to implement just one trait method to see what signature is expected
    impl RaftLogReader<config::TypeConfig> for TestStorage {
        // The compiler will tell us what signature it expects here
    }
} 