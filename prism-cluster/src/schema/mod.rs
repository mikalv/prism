//! Schema versioning and propagation for distributed Prism
//!
//! Handles schema changes consistently across cluster nodes with versioning
//! and migration support.
//!
//! # Schema Change Types
//!
//! | Type | Example | Strategy |
//! |------|---------|----------|
//! | Additive | New field | Immediate propagation |
//! | Breaking | Remove/change field | Coordinated migration |
//!
//! # Example
//!
//! ```ignore
//! use prism_cluster::schema::{SchemaRegistry, SchemaChange};
//!
//! let registry = SchemaRegistry::new(cluster_state);
//!
//! // Register new schema version
//! let version = registry.register_schema("products", schema).await?;
//!
//! // Propagate to all nodes
//! registry.propagate(version).await?;
//! ```

mod propagator;
mod registry;
mod version;

pub use propagator::{PropagationConfig, PropagationStatus, SchemaPropagator};
pub use registry::{SchemaRegistry, SchemaRegistrySnapshot};
pub use version::{ChangeType, SchemaChange, SchemaVersion, VersionedSchema};

use serde::{Deserialize, Serialize};

/// Schema propagation strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PropagationStrategy {
    /// Immediate propagation (for additive changes)
    Immediate,
    /// Versioned propagation with migration window
    #[default]
    Versioned,
    /// Manual propagation (require explicit trigger)
    Manual,
}

/// Result of a schema operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaOperationResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// New schema version (if created)
    pub version: Option<u64>,
    /// Nodes that received the change
    pub propagated_to: Vec<String>,
    /// Nodes that failed to receive the change
    pub failed_nodes: Vec<String>,
    /// Error message if failed
    pub error: Option<String>,
}

impl SchemaOperationResult {
    /// Create a successful result
    pub fn success(version: u64, propagated_to: Vec<String>) -> Self {
        Self {
            success: true,
            version: Some(version),
            propagated_to,
            failed_nodes: Vec::new(),
            error: None,
        }
    }

    /// Create a failed result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            version: None,
            propagated_to: Vec::new(),
            failed_nodes: Vec::new(),
            error: Some(error.into()),
        }
    }

    /// Create a partial success result
    pub fn partial(version: u64, propagated_to: Vec<String>, failed_nodes: Vec<String>) -> Self {
        Self {
            success: true,
            version: Some(version),
            propagated_to,
            failed_nodes,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_propagation_strategy_default() {
        assert_eq!(
            PropagationStrategy::default(),
            PropagationStrategy::Versioned
        );
    }

    #[test]
    fn test_schema_operation_result_success() {
        let result = SchemaOperationResult::success(1, vec!["node-1".into()]);
        assert!(result.success);
        assert_eq!(result.version, Some(1));
        assert!(result.failed_nodes.is_empty());
    }

    #[test]
    fn test_schema_operation_result_failure() {
        let result = SchemaOperationResult::failure("test error");
        assert!(!result.success);
        assert!(result.version.is_none());
        assert_eq!(result.error, Some("test error".into()));
    }
}
