//! Rebalancing engine implementation
//!
//! Handles the execution of rebalancing plans including shard transfers
//! with bandwidth throttling and failure handling.

use super::{OperationStatus, RebalanceOperationStatus, RebalancePhase, RebalanceStatus};
use crate::config::RebalancingConfig;
use crate::placement::{
    find_rebalance_target, ClusterState, NodeInfo, PlacementStrategy, ShardState,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Trigger for initiating rebalancing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RebalanceTrigger {
    /// Manually triggered by admin
    Manual,
    /// Triggered by node joining cluster
    NodeJoined,
    /// Triggered by node leaving cluster
    NodeLeft,
    /// Triggered by imbalance threshold exceeded
    ImbalanceThreshold,
    /// Triggered by scheduled interval
    Scheduled,
}

/// A single operation in a rebalance plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceOperation {
    /// Shard being moved
    pub shard_id: String,

    /// Source node
    pub from_node: String,

    /// Target node
    pub to_node: String,

    /// Reason for this move
    pub reason: String,

    /// Priority (lower = higher priority)
    pub priority: u32,

    /// Expected size in bytes
    pub expected_bytes: u64,
}

/// A plan for rebalancing the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalancePlan {
    /// Unique ID for this plan
    pub plan_id: String,

    /// When the plan was created
    pub created_at: u64,

    /// What triggered this rebalance
    pub trigger: RebalanceTrigger,

    /// Operations to execute
    pub operations: Vec<RebalanceOperation>,

    /// Target collection (None = all collections)
    pub collection: Option<String>,

    /// Expected total bytes to transfer
    pub total_bytes: u64,

    /// Estimated duration in seconds
    pub estimated_duration_secs: u64,
}

impl RebalancePlan {
    /// Create a new empty plan
    pub fn new(trigger: RebalanceTrigger) -> Self {
        Self {
            plan_id: uuid::Uuid::new_v4().to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            trigger,
            operations: Vec::new(),
            collection: None,
            total_bytes: 0,
            estimated_duration_secs: 0,
        }
    }

    /// Add an operation to the plan
    pub fn add_operation(&mut self, operation: RebalanceOperation) {
        self.total_bytes += operation.expected_bytes;
        self.operations.push(operation);
    }

    /// Check if the plan is empty
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Get operation count
    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }
}

/// Engine for executing rebalancing operations
pub struct RebalanceEngine {
    /// Configuration
    config: RebalancingConfig,

    /// Cluster state reference
    cluster_state: Arc<ClusterState>,

    /// Current status
    status: RwLock<RebalanceStatus>,

    /// Current plan being executed
    current_plan: RwLock<Option<RebalancePlan>>,

    /// Last rebalance time for cooldown
    last_rebalance: RwLock<Option<Instant>>,

    /// Placement strategy to use
    strategy: PlacementStrategy,
}

impl RebalanceEngine {
    /// Create a new rebalance engine
    pub fn new(
        config: RebalancingConfig,
        cluster_state: Arc<ClusterState>,
        strategy: PlacementStrategy,
    ) -> Self {
        Self {
            config,
            cluster_state,
            status: RwLock::new(RebalanceStatus::default()),
            current_plan: RwLock::new(None),
            last_rebalance: RwLock::new(None),
            strategy,
        }
    }

    /// Get current rebalance status
    pub fn status(&self) -> RebalanceStatus {
        self.status.read().clone()
    }

    /// Check if rebalancing is needed
    pub fn should_rebalance(&self) -> bool {
        if !self.config.enabled {
            return false;
        }

        // Check cooldown
        if let Some(last) = *self.last_rebalance.read() {
            if last.elapsed() < Duration::from_secs(self.config.cooldown_secs) {
                return false;
            }
        }

        // Check imbalance
        self.cluster_state
            .is_imbalanced(self.config.imbalance_threshold_percent as f64)
    }

    /// Create a rebalancing plan
    pub fn create_plan(
        &self,
        trigger: RebalanceTrigger,
        collection: Option<&str>,
    ) -> Result<RebalancePlan, String> {
        let mut plan = RebalancePlan::new(trigger);
        plan.collection = collection.map(|s| s.to_string());

        let nodes = self.cluster_state.get_healthy_nodes();
        if nodes.len() < 2 {
            return Err("Need at least 2 healthy nodes for rebalancing".to_string());
        }

        let assignments = if let Some(coll) = collection {
            self.cluster_state.get_collection_shards(coll)
        } else {
            self.cluster_state.get_all_shards()
        };

        // Find overloaded nodes
        let overloaded = self.cluster_state.find_overloaded_nodes();
        if overloaded.is_empty() {
            return Ok(plan); // Already balanced
        }

        // For each overloaded node, find shards to move
        for node_id in overloaded {
            let node_shards = self.cluster_state.get_node_shards(&node_id);

            // Try to move shards from this node
            for shard in node_shards {
                // Skip shards that are already being moved
                if shard.state != ShardState::Active {
                    continue;
                }

                // Find a target node
                match find_rebalance_target(&shard, &nodes, &assignments, &self.strategy) {
                    Ok(target) => {
                        plan.add_operation(RebalanceOperation {
                            shard_id: shard.shard_id.clone(),
                            from_node: node_id.clone(),
                            to_node: target,
                            reason: "Rebalancing overloaded node".to_string(),
                            priority: 10,
                            expected_bytes: shard.size_bytes,
                        });

                        // Limit moves per node
                        if plan.operations.len() >= self.config.max_concurrent_moves {
                            break;
                        }
                    }
                    Err(e) => {
                        debug!("Cannot find target for shard {}: {}", shard.shard_id, e);
                    }
                }
            }

            if plan.operations.len() >= self.config.max_concurrent_moves {
                break;
            }
        }

        // Estimate duration based on bandwidth limit
        if self.config.max_bytes_per_sec > 0 {
            plan.estimated_duration_secs = plan.total_bytes / self.config.max_bytes_per_sec;
        }

        Ok(plan)
    }

    /// Trigger rebalancing
    pub fn trigger(&self, trigger: RebalanceTrigger) -> Result<RebalanceStatus, String> {
        // Check if already running
        if self.status.read().in_progress {
            return Err("Rebalancing already in progress".to_string());
        }

        // Create plan
        let plan = self.create_plan(trigger, None)?;
        if plan.is_empty() {
            info!("No rebalancing needed - cluster is balanced");
            return Ok(self.status());
        }

        info!(
            "Starting rebalance with {} operations, trigger={:?}",
            plan.operation_count(),
            trigger
        );

        // Update status
        {
            let mut status = self.status.write();
            status.in_progress = true;
            status.phase = RebalancePhase::Planning;
            status.total_shards_to_move = plan.operation_count();
            status.completed_moves = 0;
            status.failed_moves = 0;
            status.started_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            );
            status.estimated_completion =
                status.started_at.map(|s| s + plan.estimated_duration_secs);
        }

        // Store plan
        *self.current_plan.write() = Some(plan);

        Ok(self.status())
    }

    /// Cancel current rebalancing
    pub fn cancel(&self) -> Result<(), String> {
        if !self.status.read().in_progress {
            return Err("No rebalancing in progress".to_string());
        }

        info!("Cancelling rebalance operation");

        // Update status
        {
            let mut status = self.status.write();
            status.in_progress = false;
            status.phase = RebalancePhase::Idle;
            for op in &mut status.current_operations {
                if op.status == OperationStatus::Pending
                    || op.status == OperationStatus::Transferring
                {
                    op.status = OperationStatus::Cancelled;
                }
            }
        }

        // Clear plan
        *self.current_plan.write() = None;

        Ok(())
    }

    /// Execute the next step of rebalancing
    /// This should be called periodically to advance the rebalance
    pub async fn step(&self) -> Result<bool, String> {
        let status = self.status.read().clone();

        if !status.in_progress {
            return Ok(false);
        }

        match status.phase {
            RebalancePhase::Planning => {
                // Move to executing
                self.status.write().phase = RebalancePhase::Executing;
                self.start_next_operations()?;
                Ok(true)
            }
            RebalancePhase::Executing => {
                // Check progress of current operations
                self.check_operation_progress().await?;

                // Start more if slots available
                self.start_next_operations()?;

                // Check if all done
                if self.all_operations_complete() {
                    self.status.write().phase = RebalancePhase::Verifying;
                }
                Ok(true)
            }
            RebalancePhase::Verifying => {
                // Verify all shards are active
                // For now, just move to finalizing
                self.status.write().phase = RebalancePhase::Finalizing;
                Ok(true)
            }
            RebalancePhase::Finalizing => {
                // Clean up and finish
                self.finalize_rebalance();
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    /// Start the next batch of operations
    fn start_next_operations(&self) -> Result<(), String> {
        let plan = self.current_plan.read();
        let plan = plan.as_ref().ok_or("No plan available")?;

        let mut status = self.status.write();
        let in_transit = status
            .current_operations
            .iter()
            .filter(|op| op.status == OperationStatus::Transferring)
            .count();

        let _slots_available = self.config.max_concurrent_moves.saturating_sub(in_transit);

        // Find operations to start
        let completed: std::collections::HashSet<_> = status
            .current_operations
            .iter()
            .map(|op| op.shard_id.clone())
            .collect();

        for op in &plan.operations {
            if completed.contains(&op.shard_id) {
                continue;
            }

            if status
                .current_operations
                .iter()
                .filter(|o| o.status == OperationStatus::Transferring)
                .count()
                >= self.config.max_concurrent_moves
            {
                break;
            }

            // Start this operation
            status.current_operations.push(RebalanceOperationStatus {
                shard_id: op.shard_id.clone(),
                from_node: op.from_node.clone(),
                to_node: op.to_node.clone(),
                progress: 0.0,
                bytes_transferred: 0,
                total_bytes: op.expected_bytes,
                status: OperationStatus::Transferring,
            });

            status.shards_in_transit += 1;

            info!(
                "Started shard transfer: {} from {} to {}",
                op.shard_id, op.from_node, op.to_node
            );
        }

        Ok(())
    }

    /// Check progress of in-flight operations
    async fn check_operation_progress(&self) -> Result<(), String> {
        // In a real implementation, this would check the actual transfer status
        // For now, we simulate progress

        let mut status = self.status.write();
        let mut newly_completed = Vec::new();

        for op in &mut status.current_operations {
            if op.status == OperationStatus::Transferring {
                // Simulate progress
                op.progress += 0.1;
                op.bytes_transferred = (op.total_bytes as f64 * op.progress.min(1.0)) as u64;

                if op.progress >= 1.0 {
                    op.status = OperationStatus::Completed;
                    newly_completed.push(op.shard_id.clone());
                }
            }
        }

        // Update counts after the loop
        let completed_count = newly_completed.len();
        status.completed_moves += completed_count;
        status.shards_in_transit = status.shards_in_transit.saturating_sub(completed_count);

        for shard_id in newly_completed {
            info!("Completed shard transfer: {}", shard_id);
        }

        Ok(())
    }

    /// Check if all operations are complete
    fn all_operations_complete(&self) -> bool {
        let plan = self.current_plan.read();
        let status = self.status.read();

        if let Some(plan) = plan.as_ref() {
            status.completed_moves + status.failed_moves >= plan.operation_count()
        } else {
            true
        }
    }

    /// Finalize the rebalance operation
    fn finalize_rebalance(&self) {
        let mut status = self.status.write();
        status.in_progress = false;

        if status.failed_moves > 0 {
            status.phase = RebalancePhase::Failed;
            warn!("Rebalance completed with {} failures", status.failed_moves);
        } else {
            status.phase = RebalancePhase::Completed;
            info!(
                "Rebalance completed successfully: {} shards moved",
                status.completed_moves
            );
        }

        // Update cooldown
        *self.last_rebalance.write() = Some(Instant::now());

        // Clear plan
        drop(status);
        *self.current_plan.write() = None;
    }

    /// Plan rebalancing for a node leaving the cluster
    pub fn plan_for_node_removal(&self, node_id: &str) -> Result<RebalancePlan, String> {
        let mut plan = RebalancePlan::new(RebalanceTrigger::NodeLeft);

        let nodes = self.cluster_state.get_healthy_nodes();
        let remaining_nodes: Vec<_> = nodes
            .iter()
            .filter(|n| n.node_id != node_id)
            .cloned()
            .collect();

        if remaining_nodes.is_empty() {
            return Err("No remaining nodes available".to_string());
        }

        // Find all shards on the departing node
        let shards = self.cluster_state.get_node_shards(node_id);
        let all_assignments = self.cluster_state.get_all_shards();

        for shard in shards {
            // Find a new home for this shard
            match find_rebalance_target(&shard, &remaining_nodes, &all_assignments, &self.strategy)
            {
                Ok(target) => {
                    plan.add_operation(RebalanceOperation {
                        shard_id: shard.shard_id.clone(),
                        from_node: node_id.to_string(),
                        to_node: target,
                        reason: format!("Node {} leaving cluster", node_id),
                        priority: 1, // High priority
                        expected_bytes: shard.size_bytes,
                    });
                }
                Err(e) => {
                    warn!(
                        "Cannot find target for shard {} during node removal: {}",
                        shard.shard_id, e
                    );
                }
            }
        }

        Ok(plan)
    }

    /// Plan rebalancing for a node joining the cluster
    pub fn plan_for_node_addition(&self, new_node: &NodeInfo) -> Result<RebalancePlan, String> {
        let mut plan = RebalancePlan::new(RebalanceTrigger::NodeJoined);

        // Find overloaded nodes that could donate shards
        let overloaded = self.cluster_state.find_overloaded_nodes();

        if overloaded.is_empty() {
            return Ok(plan); // Cluster already balanced
        }

        let all_assignments = self.cluster_state.get_all_shards();
        let _nodes = self.cluster_state.get_healthy_nodes();

        // Move some shards from overloaded nodes to the new node
        let target_shards = self.config.max_concurrent_moves.max(1);
        let mut moved = 0;

        for node_id in overloaded {
            if moved >= target_shards {
                break;
            }

            let shards = self.cluster_state.get_node_shards(&node_id);
            for shard in shards {
                if moved >= target_shards {
                    break;
                }

                if shard.state != ShardState::Active {
                    continue;
                }

                // Verify the new node satisfies spread constraints
                if let Ok(target) = find_rebalance_target(
                    &shard,
                    std::slice::from_ref(new_node),
                    &all_assignments,
                    &self.strategy,
                ) {
                    plan.add_operation(RebalanceOperation {
                        shard_id: shard.shard_id.clone(),
                        from_node: node_id.clone(),
                        to_node: target,
                        reason: format!("Distributing to new node {}", new_node.node_id),
                        priority: 10,
                        expected_bytes: shard.size_bytes,
                    });
                    moved += 1;
                }
            }
        }

        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeTopology;
    use crate::placement::{ShardAssignment, SpreadLevel};
    use std::collections::HashMap;

    fn make_test_engine() -> (RebalanceEngine, Arc<ClusterState>) {
        let state = Arc::new(ClusterState::new());
        let config = RebalancingConfig::default();
        let strategy = PlacementStrategy::default();
        let engine = RebalanceEngine::new(config, Arc::clone(&state), strategy);
        (engine, state)
    }

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
    fn test_create_empty_plan() {
        let (engine, state) = make_test_engine();

        // Add balanced nodes
        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));
        state.assign_shard(ShardAssignment::new("test", 0, "node-1"));
        state.assign_shard(ShardAssignment::new("test", 1, "node-2"));

        let plan = engine.create_plan(RebalanceTrigger::Manual, None).unwrap();
        assert!(plan.is_empty()); // Already balanced
    }

    #[test]
    fn test_status_default() {
        let (engine, _) = make_test_engine();
        let status = engine.status();

        assert!(!status.in_progress);
        assert_eq!(status.phase, RebalancePhase::Idle);
    }

    #[test]
    fn test_should_rebalance_disabled() {
        let state = Arc::new(ClusterState::new());
        let config = RebalancingConfig {
            enabled: false,
            ..Default::default()
        };

        let engine = RebalanceEngine::new(config, state, PlacementStrategy::default());
        assert!(!engine.should_rebalance());
    }

    #[test]
    fn test_rebalance_plan_new() {
        let plan = RebalancePlan::new(RebalanceTrigger::Manual);
        assert!(plan.is_empty());
        assert_eq!(plan.operation_count(), 0);
        assert_eq!(plan.total_bytes, 0);
        assert_eq!(plan.trigger, RebalanceTrigger::Manual);
        assert!(plan.collection.is_none());
        assert!(!plan.plan_id.is_empty());
        assert!(plan.created_at > 0);
    }

    #[test]
    fn test_rebalance_plan_add_operation() {
        let mut plan = RebalancePlan::new(RebalanceTrigger::NodeLeft);
        assert!(plan.is_empty());

        plan.add_operation(RebalanceOperation {
            shard_id: "shard-0".to_string(),
            from_node: "node-1".to_string(),
            to_node: "node-2".to_string(),
            reason: "test".to_string(),
            priority: 1,
            expected_bytes: 1000,
        });

        assert!(!plan.is_empty());
        assert_eq!(plan.operation_count(), 1);
        assert_eq!(plan.total_bytes, 1000);

        plan.add_operation(RebalanceOperation {
            shard_id: "shard-1".to_string(),
            from_node: "node-1".to_string(),
            to_node: "node-3".to_string(),
            reason: "test".to_string(),
            priority: 2,
            expected_bytes: 2000,
        });

        assert_eq!(plan.operation_count(), 2);
        assert_eq!(plan.total_bytes, 3000);
    }

    #[test]
    fn test_create_plan_insufficient_nodes() {
        let (engine, state) = make_test_engine();

        // Only one node - should fail
        state.register_node(make_node_info("node-1", "zone-a"));
        state.assign_shard(ShardAssignment::new("test", 0, "node-1"));

        let result = engine.create_plan(RebalanceTrigger::Manual, None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Need at least 2 healthy nodes"));
    }

    #[test]
    fn test_create_plan_with_collection_filter() {
        let (engine, state) = make_test_engine();

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));
        state.assign_shard(ShardAssignment::new("col-a", 0, "node-1"));
        state.assign_shard(ShardAssignment::new("col-b", 0, "node-2"));

        let plan = engine
            .create_plan(RebalanceTrigger::Manual, Some("col-a"))
            .unwrap();
        assert_eq!(plan.collection, Some("col-a".to_string()));
    }

    fn make_imbalanced_engine() -> (RebalanceEngine, Arc<ClusterState>) {
        let state = Arc::new(ClusterState::with_heartbeat_timeout(3600));
        let config = RebalancingConfig {
            enabled: true,
            imbalance_threshold_percent: 10,
            max_concurrent_moves: 2,
            max_bytes_per_sec: 1000,
            cooldown_secs: 0,
        };
        let strategy = PlacementStrategy {
            spread_across: SpreadLevel::None,
            ..Default::default()
        };
        let engine = RebalanceEngine::new(config, Arc::clone(&state), strategy);

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));

        // Overload node-1 with 5 active shards, give node-2 just 1
        // shard_counts_by_node => {"node-1": 5, "node-2": 1}, avg = 3
        // node-1: 5 > 3*1.2 = 3.6 => overloaded
        // imbalance = (5-1)/3 * 100 = 133%
        for i in 0..5 {
            let mut shard = ShardAssignment::new("test", i, "node-1");
            shard.state = crate::placement::ShardState::Active;
            shard.size_bytes = 500;
            state.assign_shard(shard);
        }
        let mut shard = ShardAssignment::new("test", 5, "node-2");
        shard.state = crate::placement::ShardState::Active;
        shard.size_bytes = 500;
        state.assign_shard(shard);

        (engine, state)
    }

    #[test]
    fn test_should_rebalance_imbalanced() {
        let (engine, _state) = make_imbalanced_engine();
        assert!(engine.should_rebalance());
    }

    #[test]
    fn test_should_rebalance_cooldown_active() {
        let state = Arc::new(ClusterState::new());
        let config = RebalancingConfig {
            enabled: true,
            cooldown_secs: 9999,
            ..Default::default()
        };
        let engine = RebalanceEngine::new(config, Arc::clone(&state), PlacementStrategy::default());

        // Set last_rebalance to now to trigger cooldown
        *engine.last_rebalance.write() = Some(Instant::now());

        // Even if imbalanced, cooldown should prevent rebalance
        assert!(!engine.should_rebalance());
    }

    #[test]
    fn test_trigger_already_in_progress() {
        let (engine, _state) = make_imbalanced_engine();

        // First trigger should succeed
        let result = engine.trigger(RebalanceTrigger::Manual);
        assert!(result.is_ok());
        let status = result.unwrap();
        assert!(status.in_progress);
        assert_eq!(status.phase, RebalancePhase::Planning);

        // Second trigger should fail because already in progress
        let result = engine.trigger(RebalanceTrigger::Manual);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("already in progress"));
    }

    #[test]
    fn test_trigger_balanced_cluster() {
        let state = Arc::new(ClusterState::with_heartbeat_timeout(3600));
        let config = RebalancingConfig {
            enabled: true,
            ..Default::default()
        };
        let engine =
            RebalanceEngine::new(config, Arc::clone(&state), PlacementStrategy::default());

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));

        // Balanced assignment
        let mut s1 = ShardAssignment::new("test", 0, "node-1");
        s1.state = crate::placement::ShardState::Active;
        state.assign_shard(s1);
        let mut s2 = ShardAssignment::new("test", 1, "node-2");
        s2.state = crate::placement::ShardState::Active;
        state.assign_shard(s2);

        // Trigger should succeed but report no operations needed
        let result = engine.trigger(RebalanceTrigger::Manual);
        assert!(result.is_ok());
        let status = result.unwrap();
        // No moves needed because it's balanced
        assert!(!status.in_progress);
    }

    #[test]
    fn test_cancel_no_rebalancing() {
        let (engine, _state) = make_test_engine();

        let result = engine.cancel();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No rebalancing in progress"));
    }

    #[test]
    fn test_cancel_in_progress() {
        let (engine, _state) = make_imbalanced_engine();

        // Start a rebalance
        engine.trigger(RebalanceTrigger::Manual).unwrap();
        assert!(engine.status().in_progress);

        // Cancel it
        let result = engine.cancel();
        assert!(result.is_ok());

        let status = engine.status();
        assert!(!status.in_progress);
        assert_eq!(status.phase, RebalancePhase::Idle);
    }

    #[tokio::test]
    async fn test_step_not_in_progress() {
        let (engine, _state) = make_test_engine();

        let result = engine.step().await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_step_through_phases() {
        let (engine, _state) = make_imbalanced_engine();

        // Trigger rebalance to get into Planning phase
        engine.trigger(RebalanceTrigger::Manual).unwrap();
        assert_eq!(engine.status().phase, RebalancePhase::Planning);

        // Step 1: Planning -> Executing
        let continued = engine.step().await.unwrap();
        assert!(continued);
        assert_eq!(engine.status().phase, RebalancePhase::Executing);

        // Step multiple times through Executing until progress >= 1.0
        // (each step adds 0.1 progress, so 10 steps to complete)
        for _ in 0..15 {
            let _ = engine.step().await;
            if engine.status().phase != RebalancePhase::Executing {
                break;
            }
        }

        // Should have moved beyond Executing at some point
        let phase = engine.status().phase;
        assert!(
            phase == RebalancePhase::Verifying
                || phase == RebalancePhase::Finalizing
                || phase == RebalancePhase::Completed
        );
    }

    #[test]
    fn test_plan_for_node_removal_no_remaining() {
        let state = Arc::new(ClusterState::with_heartbeat_timeout(3600));
        let config = RebalancingConfig::default();
        let engine =
            RebalanceEngine::new(config, Arc::clone(&state), PlacementStrategy::default());

        // Only one node
        state.register_node(make_node_info("node-1", "zone-a"));
        state.assign_shard(ShardAssignment::new("test", 0, "node-1"));

        let result = engine.plan_for_node_removal("node-1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No remaining nodes"));
    }

    #[test]
    fn test_plan_for_node_removal_success() {
        let state = Arc::new(ClusterState::with_heartbeat_timeout(3600));
        let config = RebalancingConfig::default();
        let engine =
            RebalanceEngine::new(config, Arc::clone(&state), PlacementStrategy::default());

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));

        let mut shard = ShardAssignment::new("test", 0, "node-1");
        shard.state = crate::placement::ShardState::Active;
        shard.size_bytes = 1024;
        state.assign_shard(shard);

        let plan = engine.plan_for_node_removal("node-1").unwrap();
        assert_eq!(plan.trigger, RebalanceTrigger::NodeLeft);
        // Should have an operation to move the shard from node-1
        assert!(!plan.is_empty());
        assert_eq!(plan.operations[0].from_node, "node-1");
        assert_eq!(plan.operations[0].to_node, "node-2");
    }

    #[test]
    fn test_plan_for_node_addition_balanced() {
        let state = Arc::new(ClusterState::with_heartbeat_timeout(3600));
        let config = RebalancingConfig::default();
        let engine =
            RebalanceEngine::new(config, Arc::clone(&state), PlacementStrategy::default());

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));

        // Balanced: 1 shard each
        let mut s1 = ShardAssignment::new("test", 0, "node-1");
        s1.state = crate::placement::ShardState::Active;
        state.assign_shard(s1);
        let mut s2 = ShardAssignment::new("test", 1, "node-2");
        s2.state = crate::placement::ShardState::Active;
        state.assign_shard(s2);

        let new_node = make_node_info("node-3", "zone-c");
        let plan = engine.plan_for_node_addition(&new_node).unwrap();
        assert_eq!(plan.trigger, RebalanceTrigger::NodeJoined);
        // Cluster is balanced, no moves needed
        assert!(plan.is_empty());
    }

    #[test]
    fn test_plan_for_node_addition_imbalanced() {
        let state = Arc::new(ClusterState::with_heartbeat_timeout(3600));
        let config = RebalancingConfig {
            max_concurrent_moves: 2,
            ..Default::default()
        };
        let strategy = PlacementStrategy {
            spread_across: SpreadLevel::None,
            ..Default::default()
        };
        let engine =
            RebalanceEngine::new(config, Arc::clone(&state), strategy);

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-3", "zone-c"));

        // Put 5 on node-1, 1 on node-3 so node-1 is overloaded
        for i in 0..5 {
            let mut s = ShardAssignment::new("test", i, "node-1");
            s.state = crate::placement::ShardState::Active;
            s.size_bytes = 100;
            state.assign_shard(s);
        }
        let mut s = ShardAssignment::new("test", 5, "node-3");
        s.state = crate::placement::ShardState::Active;
        s.size_bytes = 100;
        state.assign_shard(s);

        let new_node = make_node_info("node-2", "zone-b");
        let plan = engine.plan_for_node_addition(&new_node).unwrap();
        assert_eq!(plan.trigger, RebalanceTrigger::NodeJoined);
        // Should move some shards to the new node
        assert!(!plan.is_empty());
        for op in &plan.operations {
            assert_eq!(op.to_node, "node-2");
        }
    }

    #[test]
    fn test_rebalance_trigger_serde() {
        let trigger = RebalanceTrigger::ImbalanceThreshold;
        let json = serde_json::to_string(&trigger).unwrap();
        assert_eq!(json, "\"imbalance_threshold\"");
        let deserialized: RebalanceTrigger = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, trigger);
    }

    #[test]
    fn test_rebalance_plan_estimated_duration() {
        let (engine, state) = make_test_engine();

        state.register_node(make_node_info("node-1", "zone-a"));
        state.register_node(make_node_info("node-2", "zone-b"));

        // Overload node-1
        for i in 0..6 {
            let mut shard = ShardAssignment::new("test", i, "node-1");
            shard.state = crate::placement::ShardState::Active;
            shard.size_bytes = 10_000;
            state.assign_shard(shard);
        }

        let plan = engine
            .create_plan(RebalanceTrigger::Manual, None)
            .unwrap();
        // If plan has operations, estimated_duration should be calculated
        if !plan.is_empty() {
            // max_bytes_per_sec is 104_857_600 (default), so duration will be ~0
            // because total_bytes is small
            assert!(plan.estimated_duration_secs <= 1);
        }
    }

    #[test]
    fn test_finalize_rebalance_with_failures() {
        let (engine, _state) = make_imbalanced_engine();

        // Manually set up status to simulate failed moves
        {
            let mut status = engine.status.write();
            status.in_progress = true;
            status.phase = RebalancePhase::Finalizing;
            status.failed_moves = 2;
            status.completed_moves = 3;
        }

        engine.finalize_rebalance();

        let status = engine.status();
        assert!(!status.in_progress);
        assert_eq!(status.phase, RebalancePhase::Failed);
    }

    #[test]
    fn test_finalize_rebalance_success() {
        let (engine, _state) = make_imbalanced_engine();

        {
            let mut status = engine.status.write();
            status.in_progress = true;
            status.phase = RebalancePhase::Finalizing;
            status.failed_moves = 0;
            status.completed_moves = 3;
        }

        engine.finalize_rebalance();

        let status = engine.status();
        assert!(!status.in_progress);
        assert_eq!(status.phase, RebalancePhase::Completed);
    }

    #[test]
    fn test_all_operations_complete_no_plan() {
        let (engine, _state) = make_test_engine();
        // No plan => all_operations_complete returns true
        assert!(engine.all_operations_complete());
    }
}
