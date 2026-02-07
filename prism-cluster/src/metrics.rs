//! Cluster observability metrics
//!
//! Provides Prometheus-compatible metrics for cluster operations including:
//! - Node state (alive/suspect/dead)
//! - Shard status (primary/replica/unassigned)
//! - Replication lag
//! - Rebalance operations
//! - RPC duration and errors

use crate::placement::{ClusterState, ReplicaRole, ShardState};
use crate::rebalance::RebalanceStatus;
use std::time::{Duration, Instant};

/// Record RPC call duration
pub fn record_rpc_duration(method: &str, target_node: &str, duration: Duration) {
    metrics::histogram!(
        "prism_rpc_duration_seconds",
        "method" => method.to_string(),
        "target_node" => target_node.to_string(),
    )
    .record(duration.as_secs_f64());
}

/// Record RPC call success
pub fn record_rpc_success(method: &str, target_node: &str) {
    metrics::counter!(
        "prism_rpc_requests_total",
        "method" => method.to_string(),
        "target_node" => target_node.to_string(),
        "status" => "ok",
    )
    .increment(1);
}

/// Record RPC call error
pub fn record_rpc_error(method: &str, target_node: &str, error_type: &str) {
    metrics::counter!(
        "prism_rpc_requests_total",
        "method" => method.to_string(),
        "target_node" => target_node.to_string(),
        "status" => "error",
    )
    .increment(1);

    metrics::counter!(
        "prism_rpc_errors_total",
        "method" => method.to_string(),
        "error_type" => error_type.to_string(),
    )
    .increment(1);
}

/// Record server-side RPC handler duration
pub fn record_rpc_handler_duration(method: &str, duration: Duration) {
    metrics::histogram!(
        "prism_rpc_handler_duration_seconds",
        "method" => method.to_string(),
    )
    .record(duration.as_secs_f64());
}

/// Record server-side RPC handler success
pub fn record_rpc_handler_success(method: &str) {
    metrics::counter!(
        "prism_rpc_handler_total",
        "method" => method.to_string(),
        "status" => "ok",
    )
    .increment(1);
}

/// Record server-side RPC handler error
pub fn record_rpc_handler_error(method: &str, error_type: &str) {
    metrics::counter!(
        "prism_rpc_handler_total",
        "method" => method.to_string(),
        "status" => "error",
    )
    .increment(1);

    metrics::counter!(
        "prism_rpc_handler_errors_total",
        "method" => method.to_string(),
        "error_type" => error_type.to_string(),
    )
    .increment(1);
}

/// Record rebalance operation
pub fn record_rebalance_operation(operation_type: &str) {
    metrics::counter!(
        "prism_rebalance_operations_total",
        "type" => operation_type.to_string(),
    )
    .increment(1);
}

/// Record rebalance completion
pub fn record_rebalance_completion(success: bool, duration: Duration) {
    let status = if success { "success" } else { "failure" };
    metrics::counter!(
        "prism_rebalance_completed_total",
        "status" => status.to_string(),
    )
    .increment(1);

    metrics::histogram!("prism_rebalance_duration_seconds")
        .record(duration.as_secs_f64());
}

/// Record shard transfer
pub fn record_shard_transfer(shard_id: &str, from_node: &str, to_node: &str) {
    metrics::counter!(
        "prism_shard_transfers_total",
        "shard" => shard_id.to_string(),
        "from_node" => from_node.to_string(),
        "to_node" => to_node.to_string(),
    )
    .increment(1);
}

/// Record shard transfer completion
pub fn record_shard_transfer_complete(shard_id: &str, success: bool, bytes: u64, duration: Duration) {
    let status = if success { "success" } else { "failure" };
    metrics::counter!(
        "prism_shard_transfers_completed_total",
        "shard" => shard_id.to_string(),
        "status" => status.to_string(),
    )
    .increment(1);

    if success {
        metrics::histogram!(
            "prism_shard_transfer_duration_seconds",
            "shard" => shard_id.to_string(),
        )
        .record(duration.as_secs_f64());

        metrics::histogram!(
            "prism_shard_transfer_bytes",
            "shard" => shard_id.to_string(),
        )
        .record(bytes as f64);
    }
}

/// Record replication lag for a replica shard
pub fn record_replication_lag(shard_id: &str, replica_node: &str, lag_seconds: f64) {
    metrics::gauge!(
        "prism_replication_lag_seconds",
        "shard" => shard_id.to_string(),
        "replica" => replica_node.to_string(),
    )
    .set(lag_seconds);
}

/// Update node state gauge based on health status
pub fn update_node_state(node_id: &str, reachable: bool, healthy: bool) {
    let (state_value, state_name) = if !reachable {
        (3.0, "dead")
    } else if !healthy {
        (2.0, "suspect")
    } else {
        (1.0, "alive")
    };

    metrics::gauge!(
        "prism_node_state",
        "node_id" => node_id.to_string(),
        "state" => state_name.to_string(),
    )
    .set(state_value);
}

/// Update shard status gauge
pub fn update_shard_status(shard_id: &str, state: &ShardState, role: &ReplicaRole) {
    let state_value = match state {
        ShardState::Initializing => 1.0,
        ShardState::Active => 2.0,
        ShardState::Relocating => 3.0,
        ShardState::Syncing => 4.0,
        ShardState::Deleting => 5.0,
        ShardState::Error => 6.0,
    };

    metrics::gauge!(
        "prism_shard_status",
        "shard" => shard_id.to_string(),
        "state" => format!("{:?}", state).to_lowercase(),
        "role" => format!("{:?}", role).to_lowercase(),
    )
    .set(state_value);
}

/// Record connection pool metrics
pub fn record_connection_pool_size(size: usize) {
    metrics::gauge!("prism_cluster_connections_active")
        .set(size as f64);
}

/// Record connection establishment
pub fn record_connection_established(target_node: &str) {
    metrics::counter!(
        "prism_cluster_connections_established_total",
        "target_node" => target_node.to_string(),
    )
    .increment(1);
}

/// Record connection failure
pub fn record_connection_failed(target_node: &str, error_type: &str) {
    metrics::counter!(
        "prism_cluster_connections_failed_total",
        "target_node" => target_node.to_string(),
        "error_type" => error_type.to_string(),
    )
    .increment(1);
}

/// Collect and update all cluster state metrics
pub fn update_cluster_state_metrics(cluster_state: &ClusterState) {
    let snapshot = cluster_state.snapshot();

    // Default heartbeat timeout
    const HEARTBEAT_TIMEOUT_SECS: u64 = 30;

    // Update node count gauges
    let alive_count = snapshot.nodes.values().filter(|n| n.reachable && n.is_healthy(HEARTBEAT_TIMEOUT_SECS)).count();
    let suspect_count = snapshot.nodes.values().filter(|n| n.reachable && !n.is_healthy(HEARTBEAT_TIMEOUT_SECS)).count();
    let dead_count = snapshot.nodes.values().filter(|n| !n.reachable).count();

    metrics::gauge!("prism_cluster_nodes_alive").set(alive_count as f64);
    metrics::gauge!("prism_cluster_nodes_suspect").set(suspect_count as f64);
    metrics::gauge!("prism_cluster_nodes_dead").set(dead_count as f64);
    metrics::gauge!("prism_cluster_nodes_total").set(snapshot.nodes.len() as f64);

    // Update individual node states
    for (node_id, node_state) in &snapshot.nodes {
        let healthy = node_state.is_healthy(HEARTBEAT_TIMEOUT_SECS);
        update_node_state(node_id, node_state.reachable, healthy);
    }

    // Update shard count gauges using assignments
    let active_shards = snapshot.assignments.values().filter(|s| matches!(s.state, ShardState::Active)).count();
    let relocating_shards = snapshot.assignments.values().filter(|s| matches!(s.state, ShardState::Relocating)).count();
    let initializing_shards = snapshot.assignments.values().filter(|s| matches!(s.state, ShardState::Initializing)).count();

    metrics::gauge!("prism_cluster_shards_active").set(active_shards as f64);
    metrics::gauge!("prism_cluster_shards_relocating").set(relocating_shards as f64);
    metrics::gauge!("prism_cluster_shards_initializing").set(initializing_shards as f64);
    metrics::gauge!("prism_cluster_shards_total").set(snapshot.assignments.len() as f64);

    // Update epoch
    metrics::gauge!("prism_cluster_epoch").set(snapshot.epoch as f64);
}

/// Update rebalance status metrics
pub fn update_rebalance_status_metrics(status: &RebalanceStatus) {
    metrics::gauge!("prism_rebalance_in_progress")
        .set(if status.in_progress { 1.0 } else { 0.0 });

    metrics::gauge!("prism_rebalance_shards_in_transit")
        .set(status.shards_in_transit as f64);

    metrics::gauge!("prism_rebalance_total_shards_to_move")
        .set(status.total_shards_to_move as f64);

    metrics::gauge!("prism_rebalance_completed_moves")
        .set(status.completed_moves as f64);

    metrics::gauge!("prism_rebalance_failed_moves")
        .set(status.failed_moves as f64);
}

/// Guard for timing RPC operations
pub struct RpcTimer {
    method: String,
    target_node: String,
    start: Instant,
}

impl RpcTimer {
    /// Start timing an RPC operation
    pub fn new(method: &str, target_node: &str) -> Self {
        Self {
            method: method.to_string(),
            target_node: target_node.to_string(),
            start: Instant::now(),
        }
    }

    /// Record success and duration
    pub fn success(self) {
        let duration = self.start.elapsed();
        record_rpc_duration(&self.method, &self.target_node, duration);
        record_rpc_success(&self.method, &self.target_node);
    }

    /// Record error and duration
    pub fn error(self, error_type: &str) {
        let duration = self.start.elapsed();
        record_rpc_duration(&self.method, &self.target_node, duration);
        record_rpc_error(&self.method, &self.target_node, error_type);
    }
}

/// Guard for timing RPC handlers
pub struct RpcHandlerTimer {
    method: String,
    start: Instant,
}

impl RpcHandlerTimer {
    /// Start timing an RPC handler
    pub fn new(method: &str) -> Self {
        Self {
            method: method.to_string(),
            start: Instant::now(),
        }
    }

    /// Record success and duration
    pub fn success(self) {
        let duration = self.start.elapsed();
        record_rpc_handler_duration(&self.method, duration);
        record_rpc_handler_success(&self.method);
    }

    /// Record error and duration
    pub fn error(self, error_type: &str) {
        let duration = self.start.elapsed();
        record_rpc_handler_duration(&self.method, duration);
        record_rpc_handler_error(&self.method, error_type);
    }
}

/// Record partition events (detected, healed, quorum_lost, quorum_restored)
pub fn record_partition_event(event_type: &str) {
    metrics::counter!(
        "prism_partition_events_total",
        "event" => event_type.to_string(),
    )
    .increment(1);
}

/// Update partition state gauge
pub fn update_partition_state(state: &str, has_quorum: bool) {
    let state_value = match state {
        "healthy" => 1.0,
        "partitioned" => 2.0,
        "healing" => 3.0,
        _ => 0.0,
    };

    metrics::gauge!("prism_partition_state").set(state_value);
    metrics::gauge!("prism_partition_has_quorum").set(if has_quorum { 1.0 } else { 0.0 });
}

/// Record partition duration when healed
pub fn record_partition_duration(duration: std::time::Duration) {
    metrics::histogram!("prism_partition_duration_seconds").record(duration.as_secs_f64());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeTopology;
    use crate::placement::{ClusterState, NodeInfo, ShardAssignment};
    use std::collections::HashMap;

    #[test]
    fn test_rpc_timer() {
        let timer = RpcTimer::new("search", "node-1:9200");
        std::thread::sleep(std::time::Duration::from_millis(1));
        timer.success();
    }

    #[test]
    fn test_rpc_handler_timer() {
        let timer = RpcHandlerTimer::new("index");
        std::thread::sleep(std::time::Duration::from_millis(1));
        timer.success();
    }

    #[test]
    fn test_update_cluster_state_metrics() {
        let state = ClusterState::new();

        // Register a node
        let topology = NodeTopology {
            zone: "us-east-1a".to_string(),
            rack: None,
            region: Some("us-east-1".to_string()),
            attributes: HashMap::new(),
        };
        let node_info = NodeInfo {
            node_id: "node-1".to_string(),
            address: "127.0.0.1:9200".to_string(),
            topology,
            healthy: true,
            shard_count: 0,
            disk_used_bytes: 0,
            disk_total_bytes: 0,
            index_size_bytes: 0,
        };
        state.register_node(node_info);

        // Add a shard
        let assignment = ShardAssignment::new("products", 0, "node-1");
        state.assign_shard(assignment);

        // Should not panic
        update_cluster_state_metrics(&state);
    }
}
