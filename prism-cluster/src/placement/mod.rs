//! Shard placement module for zone-aware replica distribution
//!
//! This module provides algorithms and types for placing shard replicas
//! across cluster nodes while respecting failure domain constraints.
//!
//! # Placement Strategy
//!
//! The placement algorithm follows a two-phase approach:
//! 1. **Hard constraints**: Filter nodes that satisfy required constraints
//!    (e.g., never place two replicas of the same shard in the same zone)
//! 2. **Soft constraints**: Score remaining nodes by optimization criteria
//!    (e.g., balance shard count, disk usage)
//!
//! # Example
//!
//! ```ignore
//! use prism_cluster::placement::{PlacementStrategy, SpreadLevel};
//!
//! let strategy = PlacementStrategy {
//!     spread_across: SpreadLevel::Zone,
//!     balance_by: vec![BalanceFactor::ShardCount],
//! };
//! ```

mod algorithm;
mod state;

pub use algorithm::{find_rebalance_target, place_replicas, score_node, PlacementError};
pub use state::{ClusterState, ClusterStateSnapshot, NodeState};

use crate::config::NodeTopology;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Level at which to spread replicas
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SpreadLevel {
    /// Spread across availability zones (strongest failure isolation)
    #[default]
    Zone,
    /// Spread across racks within a zone
    Rack,
    /// Spread across regions
    Region,
    /// No placement constraints
    None,
}

impl SpreadLevel {
    /// Parse from string (used in schema)
    pub fn from_strategy_string(s: &str) -> Self {
        match s {
            "zone-aware" => SpreadLevel::Zone,
            "rack-aware" => SpreadLevel::Rack,
            "region-aware" => SpreadLevel::Region,
            "none" | "" => SpreadLevel::None,
            _ => SpreadLevel::Zone, // Default to zone-aware
        }
    }
}

/// Factor to consider when balancing shards across nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BalanceFactor {
    /// Balance by number of shards on each node
    #[default]
    ShardCount,
    /// Balance by disk usage
    DiskUsage,
    /// Balance by index size (bytes)
    IndexSize,
    /// Prefer nodes with SSD storage
    PreferSsd,
}

/// Placement strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementStrategy {
    /// Level at which to spread replicas
    #[serde(default)]
    pub spread_across: SpreadLevel,

    /// Factors to consider when balancing (in priority order)
    #[serde(default = "default_balance_factors")]
    pub balance_by: Vec<BalanceFactor>,

    /// Required node attributes for placement
    #[serde(default)]
    pub required_attributes: HashMap<String, String>,

    /// Preferred node attributes (soft preference)
    #[serde(default)]
    pub preferred_attributes: HashMap<String, String>,
}

fn default_balance_factors() -> Vec<BalanceFactor> {
    vec![BalanceFactor::ShardCount]
}

impl Default for PlacementStrategy {
    fn default() -> Self {
        Self {
            spread_across: SpreadLevel::Zone,
            balance_by: default_balance_factors(),
            required_attributes: HashMap::new(),
            preferred_attributes: HashMap::new(),
        }
    }
}

/// State of a shard in the cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ShardState {
    /// Shard is being initialized
    #[default]
    Initializing,
    /// Shard is active and serving requests
    Active,
    /// Shard is being relocated to another node
    Relocating,
    /// Shard is a replica being synchronized
    Syncing,
    /// Shard is marked for deletion
    Deleting,
    /// Shard is in an error state
    Error,
}

impl ShardState {
    /// Check if the shard can serve read requests
    pub fn can_serve_reads(&self) -> bool {
        matches!(self, ShardState::Active | ShardState::Relocating)
    }

    /// Check if the shard can accept write requests
    pub fn can_serve_writes(&self) -> bool {
        matches!(self, ShardState::Active)
    }
}

/// Role of a shard replica
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ReplicaRole {
    /// Primary replica (handles writes and reads)
    #[default]
    Primary,
    /// Replica (handles reads, receives replicated writes)
    Replica,
}

/// Assignment of a shard to a node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardAssignment {
    /// Unique identifier for this shard
    pub shard_id: String,

    /// Collection this shard belongs to
    pub collection: String,

    /// Node ID of the primary replica
    pub primary_node: String,

    /// Node IDs of additional replicas
    pub replica_nodes: Vec<String>,

    /// Current state of the shard
    pub state: ShardState,

    /// Shard number (for sharded collections)
    pub shard_number: u32,

    /// Size in bytes (for rebalancing decisions)
    pub size_bytes: u64,

    /// Document count
    pub document_count: u64,

    /// Epoch/version for conflict resolution
    pub epoch: u64,
}

impl ShardAssignment {
    /// Create a new shard assignment
    pub fn new(collection: &str, shard_number: u32, primary_node: &str) -> Self {
        Self {
            shard_id: format!("{}-shard-{}", collection, shard_number),
            collection: collection.to_string(),
            primary_node: primary_node.to_string(),
            replica_nodes: Vec::new(),
            state: ShardState::Initializing,
            shard_number,
            size_bytes: 0,
            document_count: 0,
            epoch: 1,
        }
    }

    /// Get all nodes holding this shard (primary + replicas)
    pub fn all_nodes(&self) -> Vec<&str> {
        let mut nodes = vec![self.primary_node.as_str()];
        nodes.extend(self.replica_nodes.iter().map(|s| s.as_str()));
        nodes
    }

    /// Check if a node holds this shard
    pub fn is_on_node(&self, node_id: &str) -> bool {
        self.primary_node == node_id || self.replica_nodes.contains(&node_id.to_string())
    }

    /// Get the role of a specific node for this shard
    pub fn role_on_node(&self, node_id: &str) -> Option<ReplicaRole> {
        if self.primary_node == node_id {
            Some(ReplicaRole::Primary)
        } else if self.replica_nodes.contains(&node_id.to_string()) {
            Some(ReplicaRole::Replica)
        } else {
            None
        }
    }

    /// Total replica count (primary + replicas)
    pub fn replica_count(&self) -> usize {
        1 + self.replica_nodes.len()
    }
}

/// Information about a node in the cluster for placement decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique node identifier
    pub node_id: String,

    /// Node address for RPC
    pub address: String,

    /// Node topology information
    pub topology: NodeTopology,

    /// Whether the node is healthy
    pub healthy: bool,

    /// Current shard count on this node
    pub shard_count: usize,

    /// Total disk usage in bytes
    pub disk_used_bytes: u64,

    /// Total disk capacity in bytes
    pub disk_total_bytes: u64,

    /// Index size in bytes
    pub index_size_bytes: u64,

    /// Whether this node is draining (not accepting new queries)
    #[serde(default)]
    pub draining: bool,
}

impl NodeInfo {
    /// Get disk usage as a percentage
    pub fn disk_usage_percent(&self) -> f64 {
        if self.disk_total_bytes == 0 {
            0.0
        } else {
            (self.disk_used_bytes as f64 / self.disk_total_bytes as f64) * 100.0
        }
    }

    /// Check if the node has SSD storage
    pub fn has_ssd(&self) -> bool {
        self.topology.disk_type() == Some("ssd")
    }
}

/// Decision about where to place a shard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementDecision {
    /// Shard being placed
    pub shard_id: String,

    /// Selected node for primary
    pub primary_node: String,

    /// Selected nodes for replicas
    pub replica_nodes: Vec<String>,

    /// Score for this placement (higher is better)
    pub score: f64,

    /// Reason for this placement decision
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_assignment_all_nodes() {
        let mut assignment = ShardAssignment::new("test", 0, "node-1");
        assignment.replica_nodes = vec!["node-2".to_string(), "node-3".to_string()];

        let nodes = assignment.all_nodes();
        assert_eq!(nodes.len(), 3);
        assert!(nodes.contains(&"node-1"));
        assert!(nodes.contains(&"node-2"));
        assert!(nodes.contains(&"node-3"));
    }

    #[test]
    fn test_shard_assignment_role() {
        let mut assignment = ShardAssignment::new("test", 0, "node-1");
        assignment.replica_nodes = vec!["node-2".to_string()];

        assert_eq!(
            assignment.role_on_node("node-1"),
            Some(ReplicaRole::Primary)
        );
        assert_eq!(
            assignment.role_on_node("node-2"),
            Some(ReplicaRole::Replica)
        );
        assert_eq!(assignment.role_on_node("node-3"), None);
    }

    #[test]
    fn test_spread_level_from_string() {
        assert_eq!(
            SpreadLevel::from_strategy_string("zone-aware"),
            SpreadLevel::Zone
        );
        assert_eq!(
            SpreadLevel::from_strategy_string("rack-aware"),
            SpreadLevel::Rack
        );
        assert_eq!(SpreadLevel::from_strategy_string("none"), SpreadLevel::None);
        assert_eq!(
            SpreadLevel::from_strategy_string("unknown"),
            SpreadLevel::Zone
        );
    }

    #[test]
    fn test_shard_state_serving() {
        assert!(ShardState::Active.can_serve_reads());
        assert!(ShardState::Active.can_serve_writes());
        assert!(ShardState::Relocating.can_serve_reads());
        assert!(!ShardState::Relocating.can_serve_writes());
        assert!(!ShardState::Syncing.can_serve_reads());
    }
}
