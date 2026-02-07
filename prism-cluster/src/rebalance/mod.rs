//! Shard rebalancing module
//!
//! Provides automatic and manual rebalancing of shards across cluster nodes
//! to maintain even load distribution and respect placement constraints.

mod engine;

pub use engine::{RebalanceEngine, RebalanceOperation, RebalancePlan, RebalanceTrigger};

use serde::{Deserialize, Serialize};

/// Status of a rebalancing operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceStatus {
    /// Whether rebalancing is currently in progress
    pub in_progress: bool,

    /// Current phase of rebalancing
    pub phase: RebalancePhase,

    /// Number of shards being moved
    pub shards_in_transit: usize,

    /// Total shards to move in current plan
    pub total_shards_to_move: usize,

    /// Number of completed moves
    pub completed_moves: usize,

    /// Number of failed moves
    pub failed_moves: usize,

    /// Start time of current rebalance (Unix epoch)
    pub started_at: Option<u64>,

    /// Estimated completion time (Unix epoch)
    pub estimated_completion: Option<u64>,

    /// Current operations in progress
    pub current_operations: Vec<RebalanceOperationStatus>,

    /// Last error message if any
    pub last_error: Option<String>,
}

impl Default for RebalanceStatus {
    fn default() -> Self {
        Self {
            in_progress: false,
            phase: RebalancePhase::Idle,
            shards_in_transit: 0,
            total_shards_to_move: 0,
            completed_moves: 0,
            failed_moves: 0,
            started_at: None,
            estimated_completion: None,
            current_operations: Vec::new(),
            last_error: None,
        }
    }
}

/// Phase of the rebalancing process
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RebalancePhase {
    /// No rebalancing in progress
    Idle,
    /// Analyzing cluster state
    Analyzing,
    /// Planning shard moves
    Planning,
    /// Executing shard transfers
    Executing,
    /// Verifying transfers completed
    Verifying,
    /// Finalizing and cleaning up
    Finalizing,
    /// Rebalancing completed
    Completed,
    /// Rebalancing failed
    Failed,
}

/// Status of a single rebalance operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceOperationStatus {
    /// Shard being moved
    pub shard_id: String,

    /// Source node
    pub from_node: String,

    /// Target node
    pub to_node: String,

    /// Current progress (0.0 to 1.0)
    pub progress: f64,

    /// Bytes transferred
    pub bytes_transferred: u64,

    /// Total bytes to transfer
    pub total_bytes: u64,

    /// Status of this operation
    pub status: OperationStatus,
}

/// Status of a single shard move operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// Waiting to start
    Pending,
    /// Currently transferring data
    Transferring,
    /// Verifying data integrity
    Verifying,
    /// Successfully completed
    Completed,
    /// Failed with error
    Failed,
    /// Cancelled
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rebalance_status_default() {
        let status = RebalanceStatus::default();
        assert!(!status.in_progress);
        assert_eq!(status.phase, RebalancePhase::Idle);
        assert_eq!(status.shards_in_transit, 0);
    }
}
