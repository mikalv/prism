//! Health checking and failure detection for cluster nodes
//!
//! Implements a heartbeat-based failure detection system with configurable
//! thresholds and state transitions:
//!
//! ```text
//! Node states: Alive → Suspect → Dead → Removed
//!
//! Transitions:
//!   alive → suspect: Missed heartbeats (failure_threshold)
//!   suspect → dead: Timeout without recovery (suspect_timeout)
//!   suspect → alive: Heartbeat received
//!   dead → removed: Admin action or auto-cleanup
//! ```

use crate::config::{ClusterConfig, FailureAction, HealthConfig};
use crate::metrics;
use crate::placement::ClusterState;
use crate::ClusterClient;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Health state of a node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    /// Node is responding to heartbeats
    Alive,
    /// Node has missed heartbeats but may recover
    Suspect,
    /// Node is confirmed down
    Dead,
}

impl Default for HealthState {
    fn default() -> Self {
        HealthState::Alive
    }
}

impl HealthState {
    /// Get state as a string for metrics
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthState::Alive => "alive",
            HealthState::Suspect => "suspect",
            HealthState::Dead => "dead",
        }
    }
}

/// Tracked health information for a node
#[derive(Debug, Clone)]
pub struct NodeHealthInfo {
    /// Current health state
    pub state: HealthState,
    /// When this state was entered
    pub state_since: Instant,
    /// Last successful heartbeat
    pub last_heartbeat: Option<Instant>,
    /// Count of consecutive missed heartbeats
    pub missed_heartbeats: u32,
    /// Last heartbeat latency in milliseconds
    pub last_latency_ms: Option<u64>,
}

impl Default for NodeHealthInfo {
    fn default() -> Self {
        Self {
            state: HealthState::Alive,
            state_since: Instant::now(),
            last_heartbeat: Some(Instant::now()),
            missed_heartbeats: 0,
            last_latency_ms: None,
        }
    }
}

/// Event emitted when node health state changes
#[derive(Debug, Clone)]
pub struct HealthEvent {
    pub node_id: String,
    pub previous_state: HealthState,
    pub new_state: HealthState,
    pub timestamp: Instant,
}

/// Cluster health summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterHealth {
    /// Health state of each node
    pub nodes: HashMap<String, HealthState>,
    /// Number of alive nodes
    pub alive_count: usize,
    /// Number of suspect nodes
    pub suspect_count: usize,
    /// Number of dead nodes
    pub dead_count: usize,
    /// Total node count
    pub total_count: usize,
    /// Is quorum available (majority of nodes alive)
    pub quorum_available: bool,
}

impl ClusterHealth {
    /// Create from node health map
    pub fn from_nodes(nodes: &HashMap<String, NodeHealthInfo>) -> Self {
        let mut alive_count = 0;
        let mut suspect_count = 0;
        let mut dead_count = 0;

        let states: HashMap<String, HealthState> = nodes
            .iter()
            .map(|(id, info)| {
                match info.state {
                    HealthState::Alive => alive_count += 1,
                    HealthState::Suspect => suspect_count += 1,
                    HealthState::Dead => dead_count += 1,
                }
                (id.clone(), info.state)
            })
            .collect();

        let total_count = states.len();
        let quorum_available = alive_count > total_count / 2;

        Self {
            nodes: states,
            alive_count,
            suspect_count,
            dead_count,
            total_count,
            quorum_available,
        }
    }
}

/// Health checker service
///
/// Runs periodic heartbeat checks against cluster nodes and manages
/// state transitions based on configurable thresholds.
pub struct HealthChecker {
    config: HealthConfig,
    cluster_config: ClusterConfig,
    node_health: Arc<RwLock<HashMap<String, NodeHealthInfo>>>,
    cluster_state: Arc<ClusterState>,
    event_tx: broadcast::Sender<HealthEvent>,
    running: Arc<RwLock<bool>>,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new(
        config: HealthConfig,
        cluster_config: ClusterConfig,
        cluster_state: Arc<ClusterState>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            config,
            cluster_config,
            node_health: Arc::new(RwLock::new(HashMap::new())),
            cluster_state,
            event_tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Subscribe to health events
    pub fn subscribe(&self) -> broadcast::Receiver<HealthEvent> {
        self.event_tx.subscribe()
    }

    /// Get current cluster health summary
    pub fn cluster_health(&self) -> ClusterHealth {
        ClusterHealth::from_nodes(&self.node_health.read())
    }

    /// Get health info for a specific node
    pub fn node_health(&self, node_id: &str) -> Option<NodeHealthInfo> {
        self.node_health.read().get(node_id).cloned()
    }

    /// Register a node for health monitoring
    pub fn register_node(&self, node_id: &str) {
        let mut health = self.node_health.write();
        if !health.contains_key(node_id) {
            info!("Registering node {} for health monitoring", node_id);
            health.insert(node_id.to_string(), NodeHealthInfo::default());
            metrics::update_node_state(node_id, true, true);
        }
    }

    /// Unregister a node from health monitoring
    pub fn unregister_node(&self, node_id: &str) {
        info!("Unregistering node {} from health monitoring", node_id);
        self.node_health.write().remove(node_id);
    }

    /// Start the health checker background task
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        *self.running.write() = true;

        let checker = Arc::clone(&self);
        tokio::spawn(async move {
            checker.run_loop().await;
        })
    }

    /// Stop the health checker
    pub fn stop(&self) {
        *self.running.write() = false;
    }

    /// Check if health checker is running
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Main health check loop
    async fn run_loop(&self) {
        let interval = Duration::from_millis(self.config.heartbeat_interval_ms);
        let mut ticker = tokio::time::interval(interval);

        info!(
            "Health checker started with {}ms interval, failure threshold: {}, suspect timeout: {}ms",
            self.config.heartbeat_interval_ms,
            self.config.failure_threshold,
            self.config.suspect_timeout_ms
        );

        while *self.running.read() {
            ticker.tick().await;

            if !*self.running.read() {
                break;
            }

            self.check_all_nodes().await;
            self.update_state_transitions();
            self.emit_metrics();
        }

        info!("Health checker stopped");
    }

    /// Check all registered nodes
    async fn check_all_nodes(&self) {
        let node_ids: Vec<String> = self.node_health.read().keys().cloned().collect();

        for node_id in node_ids {
            // Skip self
            if node_id == self.cluster_config.node_id {
                self.record_heartbeat(&node_id, 0);
                continue;
            }

            // Find node address
            let address = self
                .cluster_state
                .get_node(&node_id)
                .map(|n| n.info.address.clone());

            if let Some(addr) = address {
                self.check_node(&node_id, &addr).await;
            }
        }
    }

    /// Check a single node with heartbeat (ping)
    async fn check_node(&self, node_id: &str, address: &str) {
        let start = Instant::now();

        // Create a client for this check
        let client = match ClusterClient::new(self.cluster_config.clone()).await {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to create client for {}: {}", node_id, e);
                self.record_missed_heartbeat(node_id);
                return;
            }
        };

        // Send ping
        match client.ping(address).await {
            Ok(_) => {
                let latency_ms = start.elapsed().as_millis() as u64;
                self.record_heartbeat(node_id, latency_ms);
                debug!("Heartbeat from {} in {}ms", node_id, latency_ms);
            }
            Err(e) => {
                debug!("Heartbeat failed for {}: {}", node_id, e);
                self.record_missed_heartbeat(node_id);
            }
        }
    }

    /// Record a successful heartbeat
    fn record_heartbeat(&self, node_id: &str, latency_ms: u64) {
        let mut health = self.node_health.write();
        if let Some(info) = health.get_mut(node_id) {
            let previous_state = info.state;

            info.last_heartbeat = Some(Instant::now());
            info.missed_heartbeats = 0;
            info.last_latency_ms = Some(latency_ms);

            // Transition suspect → alive
            if info.state == HealthState::Suspect {
                info.state = HealthState::Alive;
                info.state_since = Instant::now();

                drop(health); // Release lock before emitting event
                self.emit_state_change(node_id, previous_state, HealthState::Alive);

                // Update cluster state
                self.cluster_state.update_heartbeat(node_id);
            }

            // Record latency metric
            metrics::record_rpc_duration("heartbeat", node_id, Duration::from_millis(latency_ms));
        }
    }

    /// Record a missed heartbeat
    fn record_missed_heartbeat(&self, node_id: &str) {
        let mut health = self.node_health.write();
        if let Some(info) = health.get_mut(node_id) {
            info.missed_heartbeats += 1;
            let previous_state = info.state;

            // Check if we should transition to suspect
            if info.state == HealthState::Alive
                && info.missed_heartbeats >= self.config.failure_threshold
            {
                info.state = HealthState::Suspect;
                info.state_since = Instant::now();

                warn!(
                    "Node {} is now suspect after {} missed heartbeats",
                    node_id, info.missed_heartbeats
                );

                drop(health);
                self.emit_state_change(node_id, previous_state, HealthState::Suspect);
            }
        }
    }

    /// Update state transitions based on timeouts
    fn update_state_transitions(&self) {
        let suspect_timeout = Duration::from_millis(self.config.suspect_timeout_ms);
        let mut transitions = Vec::new();

        {
            let mut health = self.node_health.write();
            for (node_id, info) in health.iter_mut() {
                // Check suspect → dead transition
                if info.state == HealthState::Suspect {
                    let time_in_suspect = info.state_since.elapsed();
                    if time_in_suspect >= suspect_timeout {
                        let previous_state = info.state;
                        info.state = HealthState::Dead;
                        info.state_since = Instant::now();

                        warn!(
                            "Node {} is now dead after {}s in suspect state",
                            node_id,
                            time_in_suspect.as_secs()
                        );

                        transitions.push((node_id.clone(), previous_state, HealthState::Dead));
                    }
                }
            }
        }

        // Emit events and trigger actions outside of lock
        for (node_id, previous, new) in transitions {
            self.emit_state_change(&node_id, previous, new);

            // Mark unreachable in cluster state
            self.cluster_state.mark_unreachable(&node_id);

            // Handle failure action
            self.handle_node_failure(&node_id);
        }
    }

    /// Handle a node failure based on configured action
    fn handle_node_failure(&self, node_id: &str) {
        match self.config.on_failure {
            FailureAction::Rebalance => {
                info!("Triggering rebalance due to node {} failure", node_id);
                metrics::record_rebalance_operation("node_failure");
                // Rebalancing would be triggered via the rebalance engine
                // This is a notification - actual rebalancing happens elsewhere
            }
            FailureAction::AlertOnly => {
                warn!(
                    "Node {} failed - alert only mode, no automatic action",
                    node_id
                );
            }
            FailureAction::Manual => {
                warn!("Node {} failed - manual intervention required", node_id);
            }
        }
    }

    /// Emit a state change event
    fn emit_state_change(&self, node_id: &str, previous: HealthState, new: HealthState) {
        let event = HealthEvent {
            node_id: node_id.to_string(),
            previous_state: previous,
            new_state: new,
            timestamp: Instant::now(),
        };

        // Record metric
        metrics::update_node_state(node_id, new != HealthState::Dead, new == HealthState::Alive);

        // Broadcast event (ignore if no receivers)
        let _ = self.event_tx.send(event);
    }

    /// Emit current health metrics
    fn emit_metrics(&self) {
        let health = self.cluster_health();
        metrics::record_connection_pool_size(health.alive_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeTopology;
    use crate::placement::NodeInfo;
    use std::collections::HashMap;

    fn make_config() -> (HealthConfig, ClusterConfig) {
        let health = HealthConfig {
            heartbeat_interval_ms: 100,
            failure_threshold: 3,
            suspect_timeout_ms: 500,
            on_failure: FailureAction::AlertOnly,
        };
        let cluster = ClusterConfig::default();
        (health, cluster)
    }

    #[test]
    fn test_health_state_default() {
        assert_eq!(HealthState::default(), HealthState::Alive);
    }

    #[test]
    fn test_node_health_info_default() {
        let info = NodeHealthInfo::default();
        assert_eq!(info.state, HealthState::Alive);
        assert_eq!(info.missed_heartbeats, 0);
        assert!(info.last_heartbeat.is_some());
    }

    #[test]
    fn test_cluster_health_from_nodes() {
        let mut nodes = HashMap::new();
        nodes.insert("node-1".to_string(), NodeHealthInfo::default());
        nodes.insert(
            "node-2".to_string(),
            NodeHealthInfo {
                state: HealthState::Suspect,
                ..Default::default()
            },
        );
        nodes.insert(
            "node-3".to_string(),
            NodeHealthInfo {
                state: HealthState::Dead,
                ..Default::default()
            },
        );

        let health = ClusterHealth::from_nodes(&nodes);
        assert_eq!(health.alive_count, 1);
        assert_eq!(health.suspect_count, 1);
        assert_eq!(health.dead_count, 1);
        assert_eq!(health.total_count, 3);
        assert!(!health.quorum_available); // 1 alive out of 3 is not majority
    }

    #[test]
    fn test_quorum_calculation() {
        let mut nodes = HashMap::new();
        nodes.insert("node-1".to_string(), NodeHealthInfo::default());
        nodes.insert("node-2".to_string(), NodeHealthInfo::default());
        nodes.insert(
            "node-3".to_string(),
            NodeHealthInfo {
                state: HealthState::Dead,
                ..Default::default()
            },
        );

        let health = ClusterHealth::from_nodes(&nodes);
        assert!(health.quorum_available); // 2 alive out of 3 is majority
    }

    #[test]
    fn test_register_unregister_node() {
        let (health_config, cluster_config) = make_config();
        let cluster_state = Arc::new(ClusterState::new());
        let checker = HealthChecker::new(health_config, cluster_config, cluster_state);

        checker.register_node("node-1");
        assert!(checker.node_health("node-1").is_some());

        checker.unregister_node("node-1");
        assert!(checker.node_health("node-1").is_none());
    }

    #[test]
    fn test_record_heartbeat_resets_missed_count() {
        let (health_config, cluster_config) = make_config();
        let cluster_state = Arc::new(ClusterState::new());

        // Register node in cluster state
        let node_info = NodeInfo {
            node_id: "node-1".to_string(),
            address: "127.0.0.1:9080".to_string(),
            topology: NodeTopology::default(),
            healthy: true,
            shard_count: 0,
            disk_used_bytes: 0,
            disk_total_bytes: 0,
            index_size_bytes: 0,
        };
        cluster_state.register_node(node_info);

        let checker = HealthChecker::new(health_config, cluster_config, cluster_state);
        checker.register_node("node-1");

        // Simulate missed heartbeats
        checker.record_missed_heartbeat("node-1");
        checker.record_missed_heartbeat("node-1");

        let info = checker.node_health("node-1").unwrap();
        assert_eq!(info.missed_heartbeats, 2);

        // Record successful heartbeat
        checker.record_heartbeat("node-1", 10);

        let info = checker.node_health("node-1").unwrap();
        assert_eq!(info.missed_heartbeats, 0);
        assert_eq!(info.last_latency_ms, Some(10));
    }

    #[test]
    fn test_transition_to_suspect() {
        let (mut health_config, cluster_config) = make_config();
        health_config.failure_threshold = 2; // Become suspect after 2 misses
        let cluster_state = Arc::new(ClusterState::new());
        let checker = HealthChecker::new(health_config, cluster_config, cluster_state);
        checker.register_node("node-1");

        // First miss
        checker.record_missed_heartbeat("node-1");
        assert_eq!(
            checker.node_health("node-1").unwrap().state,
            HealthState::Alive
        );

        // Second miss - should transition to suspect
        checker.record_missed_heartbeat("node-1");
        assert_eq!(
            checker.node_health("node-1").unwrap().state,
            HealthState::Suspect
        );
    }
}
