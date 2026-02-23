//! Network partition detection and handling
//!
//! Detects network partitions (split-brain scenarios) and enforces
//! consistency policies based on configuration.
//!
//! # Partition States
//!
//! ```text
//! Healthy: All/most nodes reachable, quorum available
//!     ↓ (nodes become unreachable)
//! Partitioned: Some nodes unreachable
//!     - Majority partition: Has quorum, can accept writes
//!     - Minority partition: No quorum, read-only or reject all
//!     ↓ (connectivity restored)
//! Healing: Reconciling divergent state
//!     ↓ (reconciliation complete)
//! Healthy
//! ```
//!
//! # Example
//!
//! ```ignore
//! use prism_cluster::partition::{PartitionDetector, PartitionState};
//!
//! let detector = PartitionDetector::new(config, health_checker);
//!
//! // Check if writes are allowed
//! if detector.can_accept_writes() {
//!     // Proceed with write
//! } else {
//!     // Reject or queue write
//! }
//! ```

use crate::config::{ClusterConfig, ConflictResolution, ConsistencyConfig, PartitionBehavior};
use crate::health::{ClusterHealth, HealthChecker, HealthState};
use crate::metrics;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Current partition state of the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PartitionState {
    /// Cluster is healthy, all nodes reachable
    Healthy {
        /// Number of reachable nodes
        node_count: usize,
    },

    /// Cluster is partitioned
    Partitioned {
        /// Nodes we can reach
        reachable_nodes: Vec<String>,
        /// Nodes we cannot reach
        unreachable_nodes: Vec<String>,
        /// Whether this partition has quorum
        has_quorum: bool,
        /// When partition was detected
        detected_at: u64, // Unix timestamp
    },

    /// Partition is healing (reconciliation in progress)
    Healing {
        /// Nodes that have reconnected
        reconnected_nodes: Vec<String>,
        /// Number of conflicts being resolved
        conflicts_pending: usize,
        /// When healing started
        started_at: u64, // Unix timestamp
    },
}

impl Default for PartitionState {
    fn default() -> Self {
        PartitionState::Healthy { node_count: 1 }
    }
}

impl PartitionState {
    /// Check if this is a healthy state
    pub fn is_healthy(&self) -> bool {
        matches!(self, PartitionState::Healthy { .. })
    }

    /// Check if this partition has quorum
    pub fn has_quorum(&self) -> bool {
        match self {
            PartitionState::Healthy { .. } => true,
            PartitionState::Partitioned { has_quorum, .. } => *has_quorum,
            PartitionState::Healing { .. } => true, // Healing implies quorum restored
        }
    }

    /// Get state as a string for metrics
    pub fn as_str(&self) -> &'static str {
        match self {
            PartitionState::Healthy { .. } => "healthy",
            PartitionState::Partitioned { .. } => "partitioned",
            PartitionState::Healing { .. } => "healing",
        }
    }
}

/// Event emitted when partition state changes
#[derive(Debug, Clone)]
pub enum PartitionEvent {
    /// Partition detected
    PartitionDetected {
        reachable_nodes: Vec<String>,
        unreachable_nodes: Vec<String>,
        has_quorum: bool,
    },

    /// Partition healed
    PartitionHealed {
        reconnected_nodes: Vec<String>,
        duration_secs: u64,
    },

    /// Quorum lost
    QuorumLost { alive_count: usize, required: usize },

    /// Quorum restored
    QuorumRestored { alive_count: usize },

    /// Conflict detected during healing
    ConflictDetected {
        document_id: String,
        collection: String,
    },

    /// Conflict resolved
    ConflictResolved {
        document_id: String,
        resolution: ConflictResolution,
    },
}

/// Partition detector and handler
pub struct PartitionDetector {
    config: ConsistencyConfig,
    #[allow(dead_code)]
    cluster_config: ClusterConfig,
    health_checker: Arc<HealthChecker>,
    state: Arc<RwLock<PartitionState>>,
    event_tx: broadcast::Sender<PartitionEvent>,
    /// Track when partition started for metrics
    partition_start: Arc<RwLock<Option<Instant>>>,
}

impl PartitionDetector {
    /// Create a new partition detector
    pub fn new(
        config: ConsistencyConfig,
        cluster_config: ClusterConfig,
        health_checker: Arc<HealthChecker>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(64);

        Self {
            config,
            cluster_config,
            health_checker,
            state: Arc::new(RwLock::new(PartitionState::default())),
            event_tx,
            partition_start: Arc::new(RwLock::new(None)),
        }
    }

    /// Subscribe to partition events
    pub fn subscribe(&self) -> broadcast::Receiver<PartitionEvent> {
        self.event_tx.subscribe()
    }

    /// Get current partition state
    pub fn state(&self) -> PartitionState {
        self.state.read().clone()
    }

    /// Check if writes can be accepted
    pub fn can_accept_writes(&self) -> bool {
        let state = self.state.read();
        match &*state {
            PartitionState::Healthy { .. } => true,
            PartitionState::Partitioned { has_quorum, .. } => {
                if *has_quorum {
                    true
                } else {
                    // Check partition behavior
                    matches!(
                        self.config.partition_behavior,
                        PartitionBehavior::ServeStale
                    )
                }
            }
            PartitionState::Healing { .. } => true,
        }
    }

    /// Check if reads can be served
    pub fn can_serve_reads(&self) -> bool {
        let state = self.state.read();
        match &*state {
            PartitionState::Healthy { .. } => true,
            PartitionState::Partitioned { has_quorum, .. } => {
                if *has_quorum {
                    true
                } else {
                    match self.config.partition_behavior {
                        PartitionBehavior::ReadOnly | PartitionBehavior::ServeStale => {
                            self.config.allow_stale_reads
                        }
                        PartitionBehavior::RejectAll => false,
                    }
                }
            }
            PartitionState::Healing { .. } => true,
        }
    }

    /// Check if quorum is satisfied for writes
    pub fn has_write_quorum(&self) -> bool {
        let health = self.health_checker.cluster_health();
        self.config
            .min_nodes_for_write
            .is_satisfied(health.alive_count, health.total_count)
    }

    /// Update partition state based on health checker
    pub fn update_state(&self) {
        let health = self.health_checker.cluster_health();
        let current_state = self.state.read().clone();

        let new_state = self.calculate_state(&health);

        // Detect state transitions
        if !matches!(
            (&current_state, &new_state),
            (
                PartitionState::Healthy { .. },
                PartitionState::Healthy { .. }
            ) | (
                PartitionState::Partitioned { .. },
                PartitionState::Partitioned { .. }
            ) | (
                PartitionState::Healing { .. },
                PartitionState::Healing { .. }
            )
        ) {
            self.handle_state_transition(&current_state, &new_state, &health);
        }

        *self.state.write() = new_state;
    }

    /// Calculate partition state from health data
    fn calculate_state(&self, health: &ClusterHealth) -> PartitionState {
        let reachable: Vec<String> = health
            .nodes
            .iter()
            .filter(|(_, state)| **state == HealthState::Alive)
            .map(|(id, _)| id.clone())
            .collect();

        let unreachable: Vec<String> = health
            .nodes
            .iter()
            .filter(|(_, state)| **state != HealthState::Alive)
            .map(|(id, _)| id.clone())
            .collect();

        if unreachable.is_empty() {
            PartitionState::Healthy {
                node_count: health.total_count,
            }
        } else {
            let has_quorum = self
                .config
                .min_nodes_for_write
                .is_satisfied(health.alive_count, health.total_count);

            PartitionState::Partitioned {
                reachable_nodes: reachable,
                unreachable_nodes: unreachable,
                has_quorum,
                detected_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            }
        }
    }

    /// Handle state transitions and emit events
    fn handle_state_transition(
        &self,
        from: &PartitionState,
        to: &PartitionState,
        health: &ClusterHealth,
    ) {
        match (from, to) {
            // Healthy -> Partitioned: partition detected
            (
                PartitionState::Healthy { .. },
                PartitionState::Partitioned {
                    reachable_nodes,
                    unreachable_nodes,
                    has_quorum,
                    ..
                },
            ) => {
                warn!(
                    "Partition detected: {} reachable, {} unreachable, quorum: {}",
                    reachable_nodes.len(),
                    unreachable_nodes.len(),
                    has_quorum
                );

                *self.partition_start.write() = Some(Instant::now());
                metrics::record_partition_event("detected");

                let _ = self.event_tx.send(PartitionEvent::PartitionDetected {
                    reachable_nodes: reachable_nodes.clone(),
                    unreachable_nodes: unreachable_nodes.clone(),
                    has_quorum: *has_quorum,
                });

                if !has_quorum {
                    let required = self
                        .config
                        .min_nodes_for_write
                        .min_nodes(health.total_count);
                    let _ = self.event_tx.send(PartitionEvent::QuorumLost {
                        alive_count: health.alive_count,
                        required,
                    });
                }
            }

            // Partitioned -> Healthy: partition healed
            (
                PartitionState::Partitioned {
                    unreachable_nodes, ..
                },
                PartitionState::Healthy { .. },
            ) => {
                let duration_secs = self
                    .partition_start
                    .read()
                    .map(|start| start.elapsed().as_secs())
                    .unwrap_or(0);

                info!(
                    "Partition healed after {}s, {} nodes reconnected",
                    duration_secs,
                    unreachable_nodes.len()
                );

                *self.partition_start.write() = None;
                metrics::record_partition_event("healed");

                let _ = self.event_tx.send(PartitionEvent::PartitionHealed {
                    reconnected_nodes: unreachable_nodes.clone(),
                    duration_secs,
                });

                // Trigger healing/reconciliation if enabled
                if self.config.auto_healing {
                    self.trigger_healing(unreachable_nodes);
                }
            }

            // Quorum status changes within partitioned state
            (
                PartitionState::Partitioned {
                    has_quorum: false, ..
                },
                PartitionState::Partitioned {
                    has_quorum: true, ..
                },
            ) => {
                info!("Quorum restored within partition");
                metrics::record_partition_event("quorum_restored");
                let _ = self.event_tx.send(PartitionEvent::QuorumRestored {
                    alive_count: health.alive_count,
                });
            }

            (
                PartitionState::Partitioned {
                    has_quorum: true, ..
                },
                PartitionState::Partitioned {
                    has_quorum: false, ..
                },
            ) => {
                warn!("Quorum lost within partition");
                metrics::record_partition_event("quorum_lost");
                let required = self
                    .config
                    .min_nodes_for_write
                    .min_nodes(health.total_count);
                let _ = self.event_tx.send(PartitionEvent::QuorumLost {
                    alive_count: health.alive_count,
                    required,
                });
            }

            _ => {}
        }
    }

    /// Trigger healing/reconciliation process
    fn trigger_healing(&self, reconnected_nodes: &[String]) {
        info!(
            "Triggering partition healing for {} nodes with {:?} resolution",
            reconnected_nodes.len(),
            self.config.conflict_resolution
        );

        // Update state to healing
        *self.state.write() = PartitionState::Healing {
            reconnected_nodes: reconnected_nodes.to_vec(),
            conflicts_pending: 0,
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        // In a full implementation, this would:
        // 1. Compare vector clocks or Raft logs
        // 2. Identify divergent writes
        // 3. Apply conflict resolution strategy
        // 4. Propagate resolved state to all nodes

        // For now, we just log and transition back to healthy
        // Real implementation would be async and take time
        debug!("Healing complete (simplified implementation)");

        // Immediately mark as healthy after "healing"
        // A real implementation would do actual reconciliation here
    }

    /// Start background partition monitoring
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let detector = Arc::clone(&self);

        // Subscribe to health events
        let mut health_rx = detector.health_checker.subscribe();

        tokio::spawn(async move {
            info!("Partition detector started");

            // Initial state update
            detector.update_state();

            // React to health changes
            loop {
                match health_rx.recv().await {
                    Ok(event) => {
                        debug!("Health event: {:?}", event);
                        detector.update_state();
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Partition detector lagged {} events", n);
                        detector.update_state();
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Health checker closed, stopping partition detector");
                        break;
                    }
                }
            }
        })
    }

    /// Get consistency config
    pub fn config(&self) -> &ConsistencyConfig {
        &self.config
    }

    /// Check if a specific node is reachable
    pub fn is_node_reachable(&self, node_id: &str) -> bool {
        let state = self.state.read();
        match &*state {
            PartitionState::Healthy { .. } => true,
            PartitionState::Partitioned {
                reachable_nodes, ..
            } => reachable_nodes.contains(&node_id.to_string()),
            PartitionState::Healing {
                reconnected_nodes, ..
            } => reconnected_nodes.contains(&node_id.to_string()),
        }
    }

    /// Get list of reachable nodes
    pub fn reachable_nodes(&self) -> Vec<String> {
        let health = self.health_checker.cluster_health();
        health
            .nodes
            .iter()
            .filter(|(_, state)| **state == HealthState::Alive)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get list of unreachable nodes
    pub fn unreachable_nodes(&self) -> Vec<String> {
        let health = self.health_checker.cluster_health();
        health
            .nodes
            .iter()
            .filter(|(_, state)| **state != HealthState::Alive)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// Builder for partition-aware operations
pub struct PartitionAwareOp<'a> {
    detector: &'a PartitionDetector,
    require_quorum: bool,
    allow_stale: bool,
}

impl<'a> PartitionAwareOp<'a> {
    /// Create a new partition-aware operation
    pub fn new(detector: &'a PartitionDetector) -> Self {
        Self {
            detector,
            require_quorum: true,
            allow_stale: detector.config.allow_stale_reads,
        }
    }

    /// Set whether quorum is required
    pub fn require_quorum(mut self, required: bool) -> Self {
        self.require_quorum = required;
        self
    }

    /// Set whether stale reads are allowed
    pub fn allow_stale(mut self, allowed: bool) -> Self {
        self.allow_stale = allowed;
        self
    }

    /// Check if a write operation can proceed
    pub fn can_write(&self) -> Result<(), PartitionError> {
        if self.require_quorum && !self.detector.has_write_quorum() {
            return Err(PartitionError::NoQuorum {
                alive: self.detector.health_checker.cluster_health().alive_count,
                required: self
                    .detector
                    .config
                    .min_nodes_for_write
                    .min_nodes(self.detector.health_checker.cluster_health().total_count),
            });
        }

        if !self.detector.can_accept_writes() {
            return Err(PartitionError::WriteRejected {
                reason: "Partition behavior does not allow writes".to_string(),
            });
        }

        Ok(())
    }

    /// Check if a read operation can proceed
    pub fn can_read(&self) -> Result<(), PartitionError> {
        if !self.detector.can_serve_reads() {
            if self.allow_stale {
                // Allow stale read
                return Ok(());
            }
            return Err(PartitionError::ReadRejected {
                reason: "Partition behavior does not allow reads".to_string(),
            });
        }

        Ok(())
    }
}

/// Errors related to partition handling
#[derive(Debug, Clone)]
pub enum PartitionError {
    /// No quorum available for writes
    NoQuorum { alive: usize, required: usize },

    /// Write rejected due to partition policy
    WriteRejected { reason: String },

    /// Read rejected due to partition policy
    ReadRejected { reason: String },

    /// Node is unreachable
    NodeUnreachable { node_id: String },
}

impl std::fmt::Display for PartitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitionError::NoQuorum { alive, required } => {
                write!(f, "No quorum: {} alive, {} required", alive, required)
            }
            PartitionError::WriteRejected { reason } => {
                write!(f, "Write rejected: {}", reason)
            }
            PartitionError::ReadRejected { reason } => {
                write!(f, "Read rejected: {}", reason)
            }
            PartitionError::NodeUnreachable { node_id } => {
                write!(f, "Node unreachable: {}", node_id)
            }
        }
    }
}

impl std::error::Error for PartitionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HealthConfig;
    use crate::placement::ClusterState;
    use crate::WriteQuorum;

    fn make_detector() -> (Arc<HealthChecker>, PartitionDetector) {
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig::default();
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            Arc::clone(&health_checker),
        );
        (health_checker, detector)
    }

    #[test]
    fn test_partition_state_default() {
        let state = PartitionState::default();
        assert!(state.is_healthy());
        assert!(state.has_quorum());
    }

    #[test]
    fn test_write_quorum_satisfied() {
        assert!(WriteQuorum::Quorum.is_satisfied(3, 5)); // 3 > 5/2
        assert!(WriteQuorum::Quorum.is_satisfied(2, 3)); // 2 > 3/2
        assert!(!WriteQuorum::Quorum.is_satisfied(2, 5)); // 2 <= 5/2
        assert!(WriteQuorum::All.is_satisfied(5, 5));
        assert!(!WriteQuorum::All.is_satisfied(4, 5));
        assert!(WriteQuorum::Count(3).is_satisfied(3, 5));
        assert!(!WriteQuorum::Count(3).is_satisfied(2, 5));
        assert!(WriteQuorum::One.is_satisfied(1, 5));
    }

    #[test]
    fn test_write_quorum_min_nodes() {
        assert_eq!(WriteQuorum::Quorum.min_nodes(5), 3);
        assert_eq!(WriteQuorum::Quorum.min_nodes(3), 2);
        assert_eq!(WriteQuorum::All.min_nodes(5), 5);
        assert_eq!(WriteQuorum::Count(3).min_nodes(5), 3);
        assert_eq!(WriteQuorum::One.min_nodes(5), 1);
    }

    #[test]
    fn test_partition_state_as_str() {
        assert_eq!(
            PartitionState::Healthy { node_count: 3 }.as_str(),
            "healthy"
        );
        assert_eq!(
            PartitionState::Partitioned {
                reachable_nodes: vec![],
                unreachable_nodes: vec![],
                has_quorum: true,
                detected_at: 0,
            }
            .as_str(),
            "partitioned"
        );
    }

    #[test]
    fn test_detector_creation() {
        let (_, detector) = make_detector();
        assert!(detector.state().is_healthy());
        assert!(detector.can_accept_writes());
        assert!(detector.can_serve_reads());
    }

    #[test]
    fn test_partition_aware_op() {
        let (health_checker, detector) = make_detector();

        // Register self as a node so there's at least one
        health_checker.register_node("self");

        let op = PartitionAwareOp::new(&detector);

        // Should succeed when healthy (single node = has quorum)
        assert!(op.can_read().is_ok());

        // For writes, single node satisfies One but not Quorum (1 > 1/2 = 1 > 0 = true)
        // Actually 1 > 0 is true, so quorum should be satisfied
        assert!(op.can_write().is_ok());
    }

    #[test]
    fn test_consistency_config_default() {
        let config = ConsistencyConfig::default();
        assert_eq!(config.min_nodes_for_write, WriteQuorum::Quorum);
        assert_eq!(config.partition_behavior, PartitionBehavior::ReadOnly);
        assert!(config.allow_stale_reads);
        assert_eq!(config.stale_read_max_age_secs, 30);
        assert!(config.auto_healing);
        assert_eq!(
            config.conflict_resolution,
            ConflictResolution::LastWriteWins
        );
    }

    // --- PartitionState tests ---

    #[test]
    fn test_partition_state_healthy_variants() {
        let state = PartitionState::Healthy { node_count: 5 };
        assert!(state.is_healthy());
        assert!(state.has_quorum());
        assert_eq!(state.as_str(), "healthy");
    }

    #[test]
    fn test_partition_state_partitioned_with_quorum() {
        let state = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into(), "n2".into(), "n3".into()],
            unreachable_nodes: vec!["n4".into(), "n5".into()],
            has_quorum: true,
            detected_at: 12345,
        };
        assert!(!state.is_healthy());
        assert!(state.has_quorum());
        assert_eq!(state.as_str(), "partitioned");
    }

    #[test]
    fn test_partition_state_partitioned_without_quorum() {
        let state = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into()],
            unreachable_nodes: vec!["n2".into(), "n3".into(), "n4".into()],
            has_quorum: false,
            detected_at: 12345,
        };
        assert!(!state.is_healthy());
        assert!(!state.has_quorum());
    }

    #[test]
    fn test_partition_state_healing() {
        let state = PartitionState::Healing {
            reconnected_nodes: vec!["n1".into(), "n2".into()],
            conflicts_pending: 3,
            started_at: 12345,
        };
        assert!(!state.is_healthy());
        assert!(state.has_quorum()); // Healing implies quorum restored
        assert_eq!(state.as_str(), "healing");
    }

    // --- PartitionError Display ---

    #[test]
    fn test_partition_error_no_quorum_display() {
        let err = PartitionError::NoQuorum {
            alive: 1,
            required: 3,
        };
        let msg = err.to_string();
        assert!(msg.contains("No quorum"));
        assert!(msg.contains("1 alive"));
        assert!(msg.contains("3 required"));
    }

    #[test]
    fn test_partition_error_write_rejected_display() {
        let err = PartitionError::WriteRejected {
            reason: "no quorum".to_string(),
        };
        assert!(err.to_string().contains("Write rejected"));
        assert!(err.to_string().contains("no quorum"));
    }

    #[test]
    fn test_partition_error_read_rejected_display() {
        let err = PartitionError::ReadRejected {
            reason: "partition policy".to_string(),
        };
        assert!(err.to_string().contains("Read rejected"));
    }

    #[test]
    fn test_partition_error_node_unreachable_display() {
        let err = PartitionError::NodeUnreachable {
            node_id: "node-42".to_string(),
        };
        assert!(err.to_string().contains("node-42"));
    }

    #[test]
    fn test_partition_error_is_std_error() {
        let err = PartitionError::NoQuorum {
            alive: 0,
            required: 3,
        };
        // Verify it implements std::error::Error
        let _: &dyn std::error::Error = &err;
    }

    // --- is_node_reachable tests ---

    #[test]
    fn test_is_node_reachable_healthy() {
        let (_, detector) = make_detector();
        // Healthy state means all nodes reachable
        assert!(detector.is_node_reachable("any-node"));
    }

    #[test]
    fn test_is_node_reachable_partitioned() {
        let (_, detector) = make_detector();

        // Manually set partitioned state
        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["node-1".to_string(), "node-2".to_string()],
            unreachable_nodes: vec!["node-3".to_string()],
            has_quorum: true,
            detected_at: 0,
        };

        assert!(detector.is_node_reachable("node-1"));
        assert!(detector.is_node_reachable("node-2"));
        assert!(!detector.is_node_reachable("node-3"));
        assert!(!detector.is_node_reachable("node-999"));
    }

    #[test]
    fn test_is_node_reachable_healing() {
        let (_, detector) = make_detector();

        *detector.state.write() = PartitionState::Healing {
            reconnected_nodes: vec!["node-1".to_string()],
            conflicts_pending: 0,
            started_at: 0,
        };

        assert!(detector.is_node_reachable("node-1"));
        assert!(!detector.is_node_reachable("node-2"));
    }

    // --- reachable_nodes / unreachable_nodes ---

    #[test]
    fn test_reachable_nodes_no_nodes() {
        let (_, detector) = make_detector();
        let nodes = detector.reachable_nodes();
        assert!(nodes.is_empty());
    }

    #[test]
    fn test_reachable_nodes_with_registered_nodes() {
        let (health_checker, detector) = make_detector();
        health_checker.register_node("node-1");
        health_checker.register_node("node-2");

        // All registered nodes start as Alive
        let reachable = detector.reachable_nodes();
        assert_eq!(reachable.len(), 2);

        let unreachable = detector.unreachable_nodes();
        assert!(unreachable.is_empty());
    }

    // --- can_accept_writes ---

    #[test]
    fn test_can_accept_writes_healthy() {
        let (_, detector) = make_detector();
        assert!(detector.can_accept_writes());
    }

    #[test]
    fn test_can_accept_writes_partitioned_with_quorum() {
        let (_, detector) = make_detector();
        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into(), "n2".into()],
            unreachable_nodes: vec!["n3".into()],
            has_quorum: true,
            detected_at: 0,
        };
        assert!(detector.can_accept_writes());
    }

    #[test]
    fn test_can_accept_writes_partitioned_no_quorum_readonly() {
        // Default partition behavior is ReadOnly, which rejects writes when no quorum
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig {
            partition_behavior: PartitionBehavior::ReadOnly,
            ..Default::default()
        };
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            health_checker,
        );

        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into()],
            unreachable_nodes: vec!["n2".into(), "n3".into()],
            has_quorum: false,
            detected_at: 0,
        };
        assert!(!detector.can_accept_writes());
    }

    #[test]
    fn test_can_accept_writes_partitioned_no_quorum_serve_stale() {
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig {
            partition_behavior: PartitionBehavior::ServeStale,
            ..Default::default()
        };
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            health_checker,
        );

        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into()],
            unreachable_nodes: vec!["n2".into(), "n3".into()],
            has_quorum: false,
            detected_at: 0,
        };
        // ServeStale allows writes even without quorum
        assert!(detector.can_accept_writes());
    }

    #[test]
    fn test_can_accept_writes_healing() {
        let (_, detector) = make_detector();
        *detector.state.write() = PartitionState::Healing {
            reconnected_nodes: vec!["n1".into()],
            conflicts_pending: 0,
            started_at: 0,
        };
        assert!(detector.can_accept_writes());
    }

    // --- can_serve_reads ---

    #[test]
    fn test_can_serve_reads_healthy() {
        let (_, detector) = make_detector();
        assert!(detector.can_serve_reads());
    }

    #[test]
    fn test_can_serve_reads_partitioned_with_quorum() {
        let (_, detector) = make_detector();
        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into(), "n2".into()],
            unreachable_nodes: vec!["n3".into()],
            has_quorum: true,
            detected_at: 0,
        };
        assert!(detector.can_serve_reads());
    }

    #[test]
    fn test_can_serve_reads_partitioned_no_quorum_reject_all() {
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig {
            partition_behavior: PartitionBehavior::RejectAll,
            allow_stale_reads: false,
            ..Default::default()
        };
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            health_checker,
        );

        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into()],
            unreachable_nodes: vec!["n2".into(), "n3".into()],
            has_quorum: false,
            detected_at: 0,
        };
        assert!(!detector.can_serve_reads());
    }

    #[test]
    fn test_can_serve_reads_partitioned_no_quorum_read_only_stale_allowed() {
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig {
            partition_behavior: PartitionBehavior::ReadOnly,
            allow_stale_reads: true,
            ..Default::default()
        };
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            health_checker,
        );

        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into()],
            unreachable_nodes: vec!["n2".into(), "n3".into()],
            has_quorum: false,
            detected_at: 0,
        };
        // ReadOnly + allow_stale_reads = reads allowed
        assert!(detector.can_serve_reads());
    }

    #[test]
    fn test_can_serve_reads_partitioned_no_quorum_read_only_stale_disallowed() {
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig {
            partition_behavior: PartitionBehavior::ReadOnly,
            allow_stale_reads: false,
            ..Default::default()
        };
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            health_checker,
        );

        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into()],
            unreachable_nodes: vec!["n2".into(), "n3".into()],
            has_quorum: false,
            detected_at: 0,
        };
        assert!(!detector.can_serve_reads());
    }

    #[test]
    fn test_can_serve_reads_healing() {
        let (_, detector) = make_detector();
        *detector.state.write() = PartitionState::Healing {
            reconnected_nodes: vec![],
            conflicts_pending: 0,
            started_at: 0,
        };
        assert!(detector.can_serve_reads());
    }

    // --- PartitionAwareOp tests ---

    #[test]
    fn test_partition_aware_op_require_quorum_toggle() {
        let (health_checker, detector) = make_detector();
        health_checker.register_node("self");

        let op = PartitionAwareOp::new(&detector).require_quorum(false);
        assert!(op.can_write().is_ok());
    }

    #[test]
    fn test_partition_aware_op_allow_stale_toggle() {
        let (health_checker, detector) = make_detector();
        health_checker.register_node("self");

        let op = PartitionAwareOp::new(&detector).allow_stale(true);
        assert!(op.can_read().is_ok());
    }

    #[test]
    fn test_partition_aware_op_can_write_no_quorum() {
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig {
            min_nodes_for_write: WriteQuorum::Count(3),
            partition_behavior: PartitionBehavior::ReadOnly,
            ..Default::default()
        };
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            Arc::clone(&health_checker),
        );

        // Register only 1 node, quorum requires 3
        health_checker.register_node("node-1");

        let op = PartitionAwareOp::new(&detector).require_quorum(true);
        let result = op.can_write();
        assert!(result.is_err());
        match result.unwrap_err() {
            PartitionError::NoQuorum { alive, required } => {
                assert_eq!(alive, 1);
                assert_eq!(required, 3);
            }
            _ => panic!("Expected NoQuorum error"),
        }
    }

    #[test]
    fn test_partition_aware_op_can_read_rejected_but_stale_allowed() {
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig {
            partition_behavior: PartitionBehavior::RejectAll,
            allow_stale_reads: false,
            ..Default::default()
        };
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            health_checker,
        );

        // Put into partitioned state with no quorum
        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into()],
            unreachable_nodes: vec!["n2".into(), "n3".into()],
            has_quorum: false,
            detected_at: 0,
        };

        // With allow_stale(true) override, reads should succeed even when detector says no
        let op = PartitionAwareOp::new(&detector).allow_stale(true);
        assert!(op.can_read().is_ok());
    }

    #[test]
    fn test_partition_aware_op_can_read_rejected_stale_not_allowed() {
        let health_config = HealthConfig::default();
        let cluster_config = ClusterConfig::default();
        let consistency_config = ConsistencyConfig {
            partition_behavior: PartitionBehavior::RejectAll,
            allow_stale_reads: false,
            ..Default::default()
        };
        let cluster_state = Arc::new(ClusterState::new());
        let health_checker = Arc::new(HealthChecker::new(
            health_config,
            cluster_config.clone(),
            cluster_state,
        ));
        let detector = PartitionDetector::new(
            consistency_config,
            cluster_config,
            health_checker,
        );

        *detector.state.write() = PartitionState::Partitioned {
            reachable_nodes: vec!["n1".into()],
            unreachable_nodes: vec!["n2".into(), "n3".into()],
            has_quorum: false,
            detected_at: 0,
        };

        let op = PartitionAwareOp::new(&detector).allow_stale(false);
        let result = op.can_read();
        assert!(result.is_err());
        match result.unwrap_err() {
            PartitionError::ReadRejected { reason } => {
                assert!(reason.contains("does not allow reads"));
            }
            _ => panic!("Expected ReadRejected error"),
        }
    }

    // --- subscribe ---

    #[test]
    fn test_detector_subscribe() {
        let (_, detector) = make_detector();
        let _rx = detector.subscribe();
        // Verify we can subscribe without panic
    }

    // --- config accessor ---

    #[test]
    fn test_detector_config() {
        let (_, detector) = make_detector();
        let config = detector.config();
        assert_eq!(config.min_nodes_for_write, WriteQuorum::Quorum);
    }

    // --- WriteQuorum edge cases ---

    #[test]
    fn test_write_quorum_one_zero_total() {
        // With 0 total nodes, One still requires 1
        assert!(!WriteQuorum::One.is_satisfied(0, 0));
        assert_eq!(WriteQuorum::One.min_nodes(0), 1);
    }

    #[test]
    fn test_write_quorum_all_zero() {
        assert!(WriteQuorum::All.is_satisfied(0, 0));
        assert_eq!(WriteQuorum::All.min_nodes(0), 0);
    }

    #[test]
    fn test_write_quorum_quorum_single_node() {
        // 1 total, quorum = 1 > 0 = true
        assert!(WriteQuorum::Quorum.is_satisfied(1, 1));
        assert_eq!(WriteQuorum::Quorum.min_nodes(1), 1);
    }
}
