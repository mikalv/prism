//! Cluster state tracking for shard assignments
//!
//! Maintains a consistent view of shard distribution across the cluster,
//! enabling placement decisions and rebalancing operations.

use super::{NodeInfo, ShardAssignment, ShardState};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// State of a single node in the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    /// Node information
    pub info: NodeInfo,

    /// Last heartbeat timestamp (Unix epoch seconds)
    pub last_heartbeat: u64,

    /// Is this node currently reachable
    pub reachable: bool,

    /// Node version
    pub version: String,

    /// Protocol version this node speaks
    #[serde(default)]
    pub protocol_version: u32,

    /// Minimum protocol version this node supports
    #[serde(default)]
    pub min_supported_version: u32,

    /// Whether this node is draining (not accepting new queries)
    #[serde(default)]
    pub draining: bool,
}

impl NodeState {
    /// Create a new node state
    pub fn new(info: NodeInfo) -> Self {
        Self {
            info,
            last_heartbeat: 0,
            reachable: true,
            version: String::new(),
            protocol_version: 0,
            min_supported_version: 0,
            draining: false,
        }
    }

    /// Update heartbeat timestamp
    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.reachable = true;
    }

    /// Check if node is considered healthy (heartbeat within threshold)
    pub fn is_healthy(&self, timeout_secs: u64) -> bool {
        if !self.reachable {
            return false;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.last_heartbeat) < timeout_secs
    }
}

/// Cluster-wide state tracking all nodes and shard assignments
#[derive(Debug)]
pub struct ClusterState {
    /// All known nodes in the cluster
    nodes: RwLock<HashMap<String, NodeState>>,

    /// All shard assignments by shard_id
    assignments: RwLock<HashMap<String, ShardAssignment>>,

    /// Epoch counter for state versioning
    epoch: RwLock<u64>,

    /// Heartbeat timeout in seconds
    heartbeat_timeout_secs: u64,
}

impl Default for ClusterState {
    fn default() -> Self {
        Self::new()
    }
}

impl ClusterState {
    /// Create a new empty cluster state
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            assignments: RwLock::new(HashMap::new()),
            epoch: RwLock::new(0),
            heartbeat_timeout_secs: 30,
        }
    }

    /// Create with custom heartbeat timeout
    pub fn with_heartbeat_timeout(timeout_secs: u64) -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            assignments: RwLock::new(HashMap::new()),
            epoch: RwLock::new(0),
            heartbeat_timeout_secs: timeout_secs,
        }
    }

    /// Get current epoch
    pub fn epoch(&self) -> u64 {
        *self.epoch.read()
    }

    /// Increment and return new epoch
    pub fn next_epoch(&self) -> u64 {
        let mut epoch = self.epoch.write();
        *epoch += 1;
        *epoch
    }

    // ========================================
    // Node Management
    // ========================================

    /// Register a node in the cluster
    pub fn register_node(&self, info: NodeInfo) {
        let mut state = NodeState::new(info.clone());
        state.update_heartbeat();
        self.nodes.write().insert(info.node_id.clone(), state);
    }

    /// Update node heartbeat
    pub fn update_heartbeat(&self, node_id: &str) -> bool {
        if let Some(node) = self.nodes.write().get_mut(node_id) {
            node.update_heartbeat();
            true
        } else {
            false
        }
    }

    /// Mark a node as unreachable
    pub fn mark_unreachable(&self, node_id: &str) {
        if let Some(node) = self.nodes.write().get_mut(node_id) {
            node.reachable = false;
        }
    }

    /// Remove a node from the cluster
    pub fn remove_node(&self, node_id: &str) -> Option<NodeState> {
        self.nodes.write().remove(node_id)
    }

    /// Get all nodes
    pub fn get_nodes(&self) -> Vec<NodeState> {
        self.nodes.read().values().cloned().collect()
    }

    /// Get all healthy nodes
    pub fn get_healthy_nodes(&self) -> Vec<NodeInfo> {
        self.nodes
            .read()
            .values()
            .filter(|n| n.is_healthy(self.heartbeat_timeout_secs))
            .map(|n| n.info.clone())
            .collect()
    }

    /// Get all available nodes (healthy AND not draining)
    pub fn get_available_nodes(&self) -> Vec<NodeInfo> {
        self.nodes
            .read()
            .values()
            .filter(|n| n.is_healthy(self.heartbeat_timeout_secs) && !n.draining)
            .map(|n| n.info.clone())
            .collect()
    }

    /// Drain a node (stop routing new queries to it)
    pub fn drain_node(&self, node_id: &str) -> bool {
        if let Some(node) = self.nodes.write().get_mut(node_id) {
            node.draining = true;
            node.info.draining = true;
            true
        } else {
            false
        }
    }

    /// Undrain a node (resume routing queries to it)
    pub fn undrain_node(&self, node_id: &str) -> bool {
        if let Some(node) = self.nodes.write().get_mut(node_id) {
            node.draining = false;
            node.info.draining = false;
            true
        } else {
            false
        }
    }

    /// Update protocol version info for a node
    pub fn update_node_version(&self, node_id: &str, protocol_version: u32, min_supported: u32) {
        if let Some(node) = self.nodes.write().get_mut(node_id) {
            node.protocol_version = protocol_version;
            node.min_supported_version = min_supported;
        }
    }

    /// Get a specific node
    pub fn get_node(&self, node_id: &str) -> Option<NodeState> {
        self.nodes.read().get(node_id).cloned()
    }

    /// Get node count
    pub fn node_count(&self) -> usize {
        self.nodes.read().len()
    }

    /// Get healthy node count
    pub fn healthy_node_count(&self) -> usize {
        self.nodes
            .read()
            .values()
            .filter(|n| n.is_healthy(self.heartbeat_timeout_secs))
            .count()
    }

    // ========================================
    // Shard Assignment Management
    // ========================================

    /// Add or update a shard assignment
    pub fn assign_shard(&self, assignment: ShardAssignment) {
        self.assignments
            .write()
            .insert(assignment.shard_id.clone(), assignment);
    }

    /// Remove a shard assignment
    pub fn remove_shard(&self, shard_id: &str) -> Option<ShardAssignment> {
        self.assignments.write().remove(shard_id)
    }

    /// Get a shard assignment
    pub fn get_shard(&self, shard_id: &str) -> Option<ShardAssignment> {
        self.assignments.read().get(shard_id).cloned()
    }

    /// Get all shard assignments
    pub fn get_all_shards(&self) -> Vec<ShardAssignment> {
        self.assignments.read().values().cloned().collect()
    }

    /// Get all shards for a collection
    pub fn get_collection_shards(&self, collection: &str) -> Vec<ShardAssignment> {
        self.assignments
            .read()
            .values()
            .filter(|a| a.collection == collection)
            .cloned()
            .collect()
    }

    /// Get all shards on a specific node
    pub fn get_node_shards(&self, node_id: &str) -> Vec<ShardAssignment> {
        self.assignments
            .read()
            .values()
            .filter(|a| a.is_on_node(node_id))
            .cloned()
            .collect()
    }

    /// Update shard state
    pub fn update_shard_state(&self, shard_id: &str, state: ShardState) -> bool {
        if let Some(shard) = self.assignments.write().get_mut(shard_id) {
            shard.state = state;
            true
        } else {
            false
        }
    }

    /// Get shard count per node
    pub fn shard_counts_by_node(&self) -> HashMap<String, usize> {
        let mut counts: HashMap<String, usize> = HashMap::new();

        for assignment in self.assignments.read().values() {
            *counts.entry(assignment.primary_node.clone()).or_insert(0) += 1;
            for replica in &assignment.replica_nodes {
                *counts.entry(replica.clone()).or_insert(0) += 1;
            }
        }

        counts
    }

    // ========================================
    // Balance Analysis
    // ========================================

    /// Calculate imbalance as a percentage
    /// Returns (min_shards, max_shards, imbalance_percent)
    pub fn calculate_imbalance(&self) -> (usize, usize, f64) {
        let counts = self.shard_counts_by_node();
        if counts.is_empty() {
            return (0, 0, 0.0);
        }

        let min = *counts.values().min().unwrap_or(&0);
        let max = *counts.values().max().unwrap_or(&0);

        if max == 0 {
            return (0, 0, 0.0);
        }

        let avg = counts.values().sum::<usize>() as f64 / counts.len() as f64;
        let imbalance = ((max - min) as f64 / avg) * 100.0;

        (min, max, imbalance)
    }

    /// Check if cluster is imbalanced beyond threshold
    pub fn is_imbalanced(&self, threshold_percent: f64) -> bool {
        let (_, _, imbalance) = self.calculate_imbalance();
        imbalance > threshold_percent
    }

    /// Find nodes with fewer shards than average
    pub fn find_underloaded_nodes(&self) -> Vec<String> {
        let counts = self.shard_counts_by_node();
        if counts.is_empty() {
            return Vec::new();
        }

        let avg = counts.values().sum::<usize>() as f64 / counts.len() as f64;

        counts
            .into_iter()
            .filter(|(_, count)| (*count as f64) < avg * 0.8)
            .map(|(node, _)| node)
            .collect()
    }

    /// Find nodes with more shards than average
    pub fn find_overloaded_nodes(&self) -> Vec<String> {
        let counts = self.shard_counts_by_node();
        if counts.is_empty() {
            return Vec::new();
        }

        let avg = counts.values().sum::<usize>() as f64 / counts.len() as f64;

        counts
            .into_iter()
            .filter(|(_, count)| (*count as f64) > avg * 1.2)
            .map(|(node, _)| node)
            .collect()
    }

    // ========================================
    // Serialization
    // ========================================

    /// Export state as a snapshot
    pub fn snapshot(&self) -> ClusterStateSnapshot {
        ClusterStateSnapshot {
            epoch: self.epoch(),
            nodes: self.nodes.read().clone(),
            assignments: self.assignments.read().clone(),
        }
    }

    /// Import state from a snapshot
    pub fn restore(&self, snapshot: ClusterStateSnapshot) {
        *self.epoch.write() = snapshot.epoch;
        *self.nodes.write() = snapshot.nodes;
        *self.assignments.write() = snapshot.assignments;
    }
}

/// Serializable snapshot of cluster state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStateSnapshot {
    pub epoch: u64,
    pub nodes: HashMap<String, NodeState>,
    pub assignments: HashMap<String, ShardAssignment>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeTopology;

    fn make_node_info(id: &str, zone: &str) -> NodeInfo {
        NodeInfo {
            node_id: id.to_string(),
            address: format!("{}:9080", id),
            topology: NodeTopology {
                zone: zone.to_string(),
                rack: None,
                region: None,
                attributes: HashMap::new(),
            },
            healthy: true,
            shard_count: 0,
            disk_used_bytes: 0,
            disk_total_bytes: 100_000_000_000,
            index_size_bytes: 0,
            draining: false,
        }
    }

    #[test]
    fn test_node_registration() {
        let state = ClusterState::new();

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));

        assert_eq!(state.node_count(), 2);
        assert!(state.get_node("node-1").is_some());
        assert!(state.get_node("node-3").is_none());
    }

    #[test]
    fn test_shard_assignment() {
        let state = ClusterState::new();

        state.register_node(make_node_info("node-1", "zone-a"));

        let mut assignment = ShardAssignment::new("test", 0, "node-1");
        assignment.replica_nodes = vec!["node-2".to_string()];

        state.assign_shard(assignment.clone());

        let retrieved = state.get_shard(&assignment.shard_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().collection, "test");
    }

    #[test]
    fn test_node_shards() {
        let state = ClusterState::new();

        state.assign_shard(ShardAssignment::new("test", 0, "node-1"));
        state.assign_shard(ShardAssignment::new("test", 1, "node-1"));
        state.assign_shard(ShardAssignment::new("test", 2, "node-2"));

        let node1_shards = state.get_node_shards("node-1");
        assert_eq!(node1_shards.len(), 2);

        let node2_shards = state.get_node_shards("node-2");
        assert_eq!(node2_shards.len(), 1);
    }

    #[test]
    fn test_imbalance_calculation() {
        let state = ClusterState::new();

        // Create imbalanced assignment: node-1 has 4 shards, node-2 has 1
        for i in 0..4 {
            state.assign_shard(ShardAssignment::new("test", i, "node-1"));
        }
        state.assign_shard(ShardAssignment::new("test", 4, "node-2"));

        let (min, max, imbalance) = state.calculate_imbalance();
        assert_eq!(min, 1);
        assert_eq!(max, 4);
        assert!(imbalance > 100.0); // Significantly imbalanced
    }

    #[test]
    fn test_snapshot_restore() {
        let state = ClusterState::new();

        state.register_node(make_node_info("node-1", "zone-a"));
        state.assign_shard(ShardAssignment::new("test", 0, "node-1"));

        let snapshot = state.snapshot();

        let new_state = ClusterState::new();
        new_state.restore(snapshot);

        assert_eq!(new_state.node_count(), 1);
        assert!(new_state.get_shard("test-shard-0").is_some());
    }

    #[test]
    fn test_drain_undrain_node() {
        let state = ClusterState::with_heartbeat_timeout(3600);

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));

        // Initially not draining
        let node = state.get_node("node-1").unwrap();
        assert!(!node.draining);

        // Drain node-1
        assert!(state.drain_node("node-1"));
        let node = state.get_node("node-1").unwrap();
        assert!(node.draining);
        assert!(node.info.draining);

        // Available nodes should exclude draining
        let available = state.get_available_nodes();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].node_id, "node-2");

        // Healthy nodes still includes draining
        let healthy = state.get_healthy_nodes();
        assert_eq!(healthy.len(), 2);

        // Undrain
        assert!(state.undrain_node("node-1"));
        let available = state.get_available_nodes();
        assert_eq!(available.len(), 2);

        // Drain nonexistent node returns false
        assert!(!state.drain_node("nonexistent"));
    }

    #[test]
    fn test_update_node_version() {
        let state = ClusterState::new();
        state.register_node(make_node_info("node-1", "zone-a"));

        // Initially version 0
        let node = state.get_node("node-1").unwrap();
        assert_eq!(node.protocol_version, 0);

        // Update version
        state.update_node_version("node-1", 2, 1);
        let node = state.get_node("node-1").unwrap();
        assert_eq!(node.protocol_version, 2);
        assert_eq!(node.min_supported_version, 1);
    }
}
