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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_type_all_variants() {
        let cases: Vec<(ClusterError, &str)> = vec![
            (ClusterError::Connection("".into()), "connection"),
            (ClusterError::Transport("".into()), "transport"),
            (ClusterError::Tls("".into()), "tls"),
            (ClusterError::Serialization("".into()), "serialization"),
            (
                ClusterError::CollectionNotFound("".into()),
                "collection_not_found",
            ),
            (ClusterError::Backend("".into()), "backend"),
            (ClusterError::InvalidQuery("".into()), "invalid_query"),
            (ClusterError::Timeout("".into()), "timeout"),
            (ClusterError::NodeUnavailable("".into()), "node_unavailable"),
            (ClusterError::Config("".into()), "config"),
            (ClusterError::Internal("".into()), "internal"),
            (ClusterError::NotImplemented("".into()), "not_implemented"),
            (ClusterError::Discovery("".into()), "discovery"),
        ];

        for (err, expected) in cases {
            assert_eq!(err.error_type(), expected, "Failed for variant {:?}", err);
        }
    }

    #[test]
    fn test_display_impl() {
        let err = ClusterError::Connection("host unreachable".into());
        assert_eq!(err.to_string(), "Connection error: host unreachable");

        let err = ClusterError::CollectionNotFound("test_idx".into());
        assert_eq!(err.to_string(), "Collection not found: test_idx");

        let err = ClusterError::Timeout("5s elapsed".into());
        assert_eq!(err.to_string(), "Timeout: 5s elapsed");
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let cluster_err: ClusterError = io_err.into();
        assert_eq!(cluster_err.error_type(), "transport");
        assert!(cluster_err.to_string().contains("refused"));
    }

    #[test]
    fn test_from_prism_error_collection_not_found() {
        let prism_err = prism::Error::CollectionNotFound("my_col".into());
        let cluster_err: ClusterError = prism_err.into();
        match cluster_err {
            ClusterError::CollectionNotFound(name) => assert_eq!(name, "my_col"),
            _ => panic!("Expected CollectionNotFound"),
        }
    }

    #[test]
    fn test_from_prism_error_backend() {
        let prism_err = prism::Error::Backend("disk full".into());
        let cluster_err: ClusterError = prism_err.into();
        match cluster_err {
            ClusterError::Backend(msg) => assert_eq!(msg, "disk full"),
            _ => panic!("Expected Backend"),
        }
    }

    #[test]
    fn test_from_prism_error_invalid_query() {
        let prism_err = prism::Error::InvalidQuery("bad syntax".into());
        let cluster_err: ClusterError = prism_err.into();
        match cluster_err {
            ClusterError::InvalidQuery(msg) => assert_eq!(msg, "bad syntax"),
            _ => panic!("Expected InvalidQuery"),
        }
    }

    #[test]
    fn test_from_prism_error_config() {
        let prism_err = prism::Error::Config("missing key".into());
        let cluster_err: ClusterError = prism_err.into();
        match cluster_err {
            ClusterError::Config(msg) => assert_eq!(msg, "missing key"),
            _ => panic!("Expected Config"),
        }
    }

    #[test]
    fn test_from_prism_error_other_becomes_internal() {
        let prism_err = prism::Error::Schema("bad schema".into());
        let cluster_err: ClusterError = prism_err.into();
        assert_eq!(cluster_err.error_type(), "internal");
    }

    #[test]
    fn test_serde_roundtrip() {
        let err = ClusterError::Timeout("deadline exceeded".into());
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: ClusterError = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.to_string(), err.to_string());
    }

    #[test]
    fn test_clone() {
        let err = ClusterError::NodeUnavailable("node-3".into());
        let cloned = err.clone();
        assert_eq!(cloned.to_string(), err.to_string());
    }
}
