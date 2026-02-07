//! Cluster-specific error types

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during cluster operations
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum ClusterError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Backend error: {0}")]
    Backend(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Node unavailable: {0}")]
    NodeUnavailable(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("Discovery error: {0}")]
    Discovery(String),
}

impl ClusterError {
    /// Get the error type as a string for metrics labeling
    pub fn error_type(&self) -> &'static str {
        match self {
            ClusterError::Connection(_) => "connection",
            ClusterError::Transport(_) => "transport",
            ClusterError::Tls(_) => "tls",
            ClusterError::Serialization(_) => "serialization",
            ClusterError::CollectionNotFound(_) => "collection_not_found",
            ClusterError::Backend(_) => "backend",
            ClusterError::InvalidQuery(_) => "invalid_query",
            ClusterError::Timeout(_) => "timeout",
            ClusterError::NodeUnavailable(_) => "node_unavailable",
            ClusterError::Config(_) => "config",
            ClusterError::Internal(_) => "internal",
            ClusterError::NotImplemented(_) => "not_implemented",
            ClusterError::Discovery(_) => "discovery",
        }
    }
}

impl From<prism::Error> for ClusterError {
    fn from(err: prism::Error) -> Self {
        match err {
            prism::Error::CollectionNotFound(name) => ClusterError::CollectionNotFound(name),
            prism::Error::Backend(msg) => ClusterError::Backend(msg),
            prism::Error::InvalidQuery(msg) => ClusterError::InvalidQuery(msg),
            prism::Error::Config(msg) => ClusterError::Config(msg),
            other => ClusterError::Internal(other.to_string()),
        }
    }
}

impl From<std::io::Error> for ClusterError {
    fn from(err: std::io::Error) -> Self {
        ClusterError::Transport(err.to_string())
    }
}

impl From<bincode::Error> for ClusterError {
    fn from(err: bincode::Error) -> Self {
        ClusterError::Serialization(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ClusterError>;
