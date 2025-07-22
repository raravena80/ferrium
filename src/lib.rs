//! Ferrite - A distributed KV storage system built with openraft
//!
//! This library provides a distributed key-value store similar to etcd or Zookeeper,
//! implemented in Rust using the openraft consensus protocol library.

pub mod client;
pub mod config;
pub mod grpc;
pub mod network;
pub mod storage;

pub use config::{KvRequest, KvResponse, KvSnapshot, Node, NodeId, TypeConfig};
pub use network::{HttpNetwork, HttpNetworkFactory, NetworkConfig};
pub use storage::{new_storage, FerriiteStorage, LogStore, StateMachineStore};

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

// Tests are in individual modules and integration tests
