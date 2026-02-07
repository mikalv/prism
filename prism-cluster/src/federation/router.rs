//! Query routing for federated search
//!
//! Determines which shards should receive a query and how documents
//! should be routed for indexing.

use crate::error::{ClusterError, Result};
use crate::placement::{ClusterState, ShardAssignment, ShardState};
use crate::types::RpcQuery;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Routing strategy for queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    /// Broadcast to all shards (default for search)
    Broadcast,
    /// Route to specific shard based on document ID hash
    HashRouting,
    /// Route to shards containing specific field value
    FieldRouting,
    /// Custom routing key provided in query
    CustomRouting,
}

impl Default for RoutingStrategy {
    fn default() -> Self {
        RoutingStrategy::Broadcast
    }
}

/// Target shard for query execution
#[derive(Debug, Clone)]
pub struct ShardTarget {
    /// Shard identifier
    pub shard_id: String,
    /// Collection name
    pub collection: String,
    /// Primary node address
    pub node_address: String,
    /// Shard number
    pub shard_number: u32,
    /// Replica addresses (for failover)
    pub replicas: Vec<String>,
}

impl ShardTarget {
    /// Create from a shard assignment
    pub fn from_assignment(assignment: &ShardAssignment, node_address: String) -> Self {
        Self {
            shard_id: assignment.shard_id.clone(),
            collection: assignment.collection.clone(),
            node_address,
            shard_number: assignment.shard_number,
            replicas: assignment.replica_nodes.clone(),
        }
    }
}

/// Result of routing decision
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    /// Target shards to query
    pub targets: Vec<ShardTarget>,
    /// Routing strategy used
    pub strategy: RoutingStrategy,
    /// Whether this is a partial routing (not all shards available)
    pub is_partial: bool,
}

/// Query router
pub struct QueryRouter {
    cluster_state: Arc<ClusterState>,
}

impl QueryRouter {
    /// Create a new query router
    pub fn new(cluster_state: Arc<ClusterState>) -> Self {
        Self { cluster_state }
    }

    /// Route a search query to appropriate shards
    pub fn route(&self, collection: &str, _query: &RpcQuery) -> Result<RoutingDecision> {
        // Get all shards for the collection
        let shards = self.cluster_state.get_collection_shards(collection);

        if shards.is_empty() {
            // No shards - might be a single-node setup
            // Return empty routing, caller should handle local execution
            return Ok(RoutingDecision {
                targets: Vec::new(),
                strategy: RoutingStrategy::Broadcast,
                is_partial: false,
            });
        }

        // Filter to active shards
        let mut targets = Vec::new();
        let mut unavailable = 0;

        for shard in &shards {
            if shard.state.can_serve_reads() {
                // Get node address from cluster state
                if let Some(node) = self.cluster_state.get_node(&shard.primary_node) {
                    targets.push(ShardTarget::from_assignment(shard, node.info.address.clone()));
                } else {
                    // Try replicas
                    let mut found = false;
                    for replica in &shard.replica_nodes {
                        if let Some(node) = self.cluster_state.get_node(replica) {
                            targets.push(ShardTarget::from_assignment(shard, node.info.address.clone()));
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        unavailable += 1;
                    }
                }
            } else {
                unavailable += 1;
            }
        }

        Ok(RoutingDecision {
            targets,
            strategy: RoutingStrategy::Broadcast,
            is_partial: unavailable > 0,
        })
    }

    /// Route a get/index/delete by document ID
    pub fn route_by_id(&self, collection: &str, id: &str) -> Result<RoutingDecision> {
        let shards = self.cluster_state.get_collection_shards(collection);

        if shards.is_empty() {
            return Ok(RoutingDecision {
                targets: Vec::new(),
                strategy: RoutingStrategy::HashRouting,
                is_partial: false,
            });
        }

        // Hash the ID to determine target shard
        let shard_count = shards.len();
        let shard_index = Self::hash_to_shard(id, shard_count);

        // Find the shard with this number
        let target_shard = shards
            .iter()
            .find(|s| s.shard_number as usize == shard_index);

        if let Some(shard) = target_shard {
            let mut targets = Vec::new();

            // Add primary
            if let Some(node) = self.cluster_state.get_node(&shard.primary_node) {
                targets.push(ShardTarget::from_assignment(shard, node.info.address.clone()));
            }

            // Add replicas for failover
            for replica in &shard.replica_nodes {
                if let Some(node) = self.cluster_state.get_node(replica) {
                    targets.push(ShardTarget::from_assignment(shard, node.info.address.clone()));
                }
            }

            Ok(RoutingDecision {
                targets,
                strategy: RoutingStrategy::HashRouting,
                is_partial: false,
            })
        } else {
            // Shard not found - this shouldn't happen
            Err(ClusterError::Internal(format!(
                "Shard {} not found for collection {}",
                shard_index, collection
            )))
        }
    }

    /// Route with custom routing key
    pub fn route_by_key(
        &self,
        collection: &str,
        routing_key: &str,
    ) -> Result<RoutingDecision> {
        // Use the routing key instead of document ID
        self.route_by_id(collection, routing_key)
    }

    /// Hash a string to a shard index
    fn hash_to_shard(key: &str, shard_count: usize) -> usize {
        if shard_count == 0 {
            return 0;
        }
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % shard_count
    }

    /// Get shard count for a collection
    pub fn shard_count(&self, collection: &str) -> usize {
        self.cluster_state.get_collection_shards(collection).len()
    }

    /// Check if all shards are available
    pub fn all_shards_available(&self, collection: &str) -> bool {
        let shards = self.cluster_state.get_collection_shards(collection);
        shards.iter().all(|s| {
            s.state.can_serve_reads()
                && self.cluster_state.get_node(&s.primary_node).is_some()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeTopology;
    use crate::placement::NodeInfo;
    use std::collections::HashMap;

    fn setup_test_state() -> Arc<ClusterState> {
        let state = Arc::new(ClusterState::new());

        // Register nodes
        for i in 1..=3 {
            let node = NodeInfo {
                node_id: format!("node-{}", i),
                address: format!("127.0.0.1:908{}", i - 1),
                topology: NodeTopology::default(),
                healthy: true,
                shard_count: 0,
                disk_used_bytes: 0,
                disk_total_bytes: 0,
                index_size_bytes: 0,
            };
            state.register_node(node);
        }

        // Assign shards
        for i in 0..3 {
            let mut shard = ShardAssignment::new("products", i, &format!("node-{}", i + 1));
            shard.state = ShardState::Active;
            state.assign_shard(shard);
        }

        state
    }

    #[test]
    fn test_hash_to_shard() {
        // Consistent hashing
        assert_eq!(QueryRouter::hash_to_shard("doc-1", 3), QueryRouter::hash_to_shard("doc-1", 3));

        // Different keys may go to different shards
        let shard1 = QueryRouter::hash_to_shard("doc-1", 10);
        let shard2 = QueryRouter::hash_to_shard("doc-2", 10);
        assert!(shard1 < 10);
        assert!(shard2 < 10);
    }

    #[test]
    fn test_route_broadcast() {
        let state = setup_test_state();
        let router = QueryRouter::new(state);

        let query = RpcQuery {
            query_string: "test".into(),
            fields: vec!["title".into()],
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
        };

        let decision = router.route("products", &query).unwrap();
        assert_eq!(decision.strategy, RoutingStrategy::Broadcast);
        assert_eq!(decision.targets.len(), 3);
        assert!(!decision.is_partial);
    }

    #[test]
    fn test_route_by_id() {
        let state = setup_test_state();
        let router = QueryRouter::new(state);

        let decision = router.route_by_id("products", "doc-123").unwrap();
        assert_eq!(decision.strategy, RoutingStrategy::HashRouting);
        assert!(!decision.targets.is_empty());
    }

    #[test]
    fn test_shard_count() {
        let state = setup_test_state();
        let router = QueryRouter::new(state);

        assert_eq!(router.shard_count("products"), 3);
        assert_eq!(router.shard_count("nonexistent"), 0);
    }

    #[test]
    fn test_routing_decision_empty() {
        let state = Arc::new(ClusterState::new());
        let router = QueryRouter::new(state);

        let query = RpcQuery {
            query_string: "test".into(),
            fields: vec![],
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
        };

        let decision = router.route("products", &query).unwrap();
        assert!(decision.targets.is_empty());
    }
}
