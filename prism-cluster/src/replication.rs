//! Async primary→replica write replication
//!
//! After a primary node successfully indexes or deletes documents,
//! the `ReplicationManager` fans out the same operation to all replica
//! nodes asynchronously. This is fire-and-forget in Phase 1 — replica
//! failures are logged but do not block the primary response.

use crate::client::ClusterClient;
use crate::config::ReplicationConfig;
use crate::placement::ClusterState;
use crate::types::RpcDocument;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// Describes a write operation to replicate to replicas.
#[derive(Debug, Clone)]
pub enum ReplicationOp {
    /// Index (upsert) documents into a collection.
    Index {
        collection: String,
        docs: Vec<RpcDocument>,
    },
    /// Delete documents by ID from a collection.
    Delete {
        collection: String,
        ids: Vec<String>,
    },
}

/// Tracks replication metrics.
#[derive(Debug, Default)]
pub struct ReplicationMetrics {
    pub ops_sent: AtomicU64,
    pub ops_succeeded: AtomicU64,
    pub ops_failed: AtomicU64,
}

/// Manages async replication of writes from primary to replica nodes.
pub struct ReplicationManager {
    client: Arc<ClusterClient>,
    cluster_state: Arc<ClusterState>,
    config: ReplicationConfig,
    node_id: String,
    metrics: Arc<ReplicationMetrics>,
    /// Limits concurrent in-flight replication tasks.
    semaphore: Arc<Semaphore>,
}

impl ReplicationManager {
    /// Create a new ReplicationManager.
    ///
    /// - `client`: ClusterClient for sending RPCs to replica nodes
    /// - `cluster_state`: shared cluster state for looking up shard assignments
    /// - `config`: replication configuration
    /// - `node_id`: this node's ID (to determine primary status)
    pub fn new(
        client: Arc<ClusterClient>,
        cluster_state: Arc<ClusterState>,
        config: ReplicationConfig,
        node_id: String,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_replications));
        Self {
            client,
            cluster_state,
            config,
            node_id,
            metrics: Arc::new(ReplicationMetrics::default()),
            semaphore,
        }
    }

    /// Check if this node is primary for any shard of the given collection.
    /// Returns the shard ID if so.
    pub fn primary_shard_for_collection(&self, collection: &str) -> Option<String> {
        let shards = self.cluster_state.get_collection_shards(collection);
        for shard in shards {
            if shard.primary_node == self.node_id {
                return Some(shard.shard_id);
            }
        }
        None
    }

    /// Get the addresses of all replica nodes for the given shard.
    fn replica_addresses(&self, shard_id: &str) -> Vec<(String, String)> {
        let shard = match self.cluster_state.get_shard(shard_id) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let mut addrs = Vec::new();
        for replica_node_id in &shard.replica_nodes {
            if let Some(node_state) = self.cluster_state.get_node(replica_node_id) {
                addrs.push((replica_node_id.clone(), node_state.info.address.clone()));
            } else {
                warn!(
                    "Replica node {} for shard {} not found in cluster state",
                    replica_node_id, shard_id
                );
            }
        }
        addrs
    }

    /// Replicate a write operation to all replicas of the given shard.
    ///
    /// This is async and non-blocking — spawns tokio tasks for each replica.
    /// Does NOT block the primary response path.
    pub fn replicate_write(&self, shard_id: &str, op: ReplicationOp) {
        if !self.config.enabled {
            return;
        }

        let replicas = self.replica_addresses(shard_id);
        if replicas.is_empty() {
            debug!("No replicas for shard {}, skipping replication", shard_id);
            return;
        }

        let replica_count = replicas.len();
        info!(
            "Replicating write to {} replica(s) for shard {}",
            replica_count, shard_id
        );

        for (replica_node_id, replica_addr) in replicas {
            let client = Arc::clone(&self.client);
            let op = op.clone();
            let metrics = Arc::clone(&self.metrics);
            let semaphore = Arc::clone(&self.semaphore);
            let timeout = Duration::from_millis(self.config.replication_timeout_ms);
            let max_retries = self.config.retry_count;
            let retry_delay = Duration::from_millis(self.config.retry_delay_ms);
            let shard_id = shard_id.to_string();

            tokio::spawn(async move {
                // Acquire semaphore permit to limit concurrent replication tasks
                let _permit = semaphore.acquire().await;
                metrics.ops_sent.fetch_add(1, Ordering::Relaxed);

                let mut last_err = None;
                // 1 initial attempt + max_retries retries
                let total_attempts = 1 + max_retries;
                for attempt in 0..total_attempts {
                    if attempt > 0 {
                        debug!(
                            "Retry {}/{} replicating to {} for shard {}",
                            attempt, max_retries, replica_node_id, shard_id
                        );
                        tokio::time::sleep(retry_delay).await;
                    }

                    let result = tokio::time::timeout(timeout, async {
                        match &op {
                            ReplicationOp::Index { collection, docs } => {
                                client.index(&replica_addr, collection, docs.clone()).await
                            }
                            ReplicationOp::Delete { collection, ids } => {
                                client.delete(&replica_addr, collection, ids.clone()).await
                            }
                        }
                    })
                    .await;

                    match result {
                        Ok(Ok(())) => {
                            metrics.ops_succeeded.fetch_add(1, Ordering::Relaxed);
                            debug!(
                                "Replication to {} succeeded for shard {}",
                                replica_node_id, shard_id
                            );
                            return;
                        }
                        Ok(Err(e)) => {
                            last_err = Some(format!("{}", e));
                        }
                        Err(_) => {
                            last_err = Some("timeout".to_string());
                        }
                    }
                }

                // All attempts exhausted
                metrics.ops_failed.fetch_add(1, Ordering::Relaxed);
                warn!(
                    "Replication to {} failed for shard {} after {} retries: {}",
                    replica_node_id,
                    shard_id,
                    max_retries,
                    last_err.unwrap_or_default()
                );
            });
        }
    }

    /// Get a snapshot of replication metrics.
    pub fn metrics(&self) -> &Arc<ReplicationMetrics> {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replication_op_clone() {
        let op = ReplicationOp::Index {
            collection: "test".to_string(),
            docs: vec![],
        };
        let cloned = op.clone();
        match cloned {
            ReplicationOp::Index { collection, docs } => {
                assert_eq!(collection, "test");
                assert!(docs.is_empty());
            }
            _ => panic!("Expected Index"),
        }
    }

    #[test]
    fn test_replication_metrics_default() {
        let m = ReplicationMetrics::default();
        assert_eq!(m.ops_sent.load(Ordering::Relaxed), 0);
        assert_eq!(m.ops_succeeded.load(Ordering::Relaxed), 0);
        assert_eq!(m.ops_failed.load(Ordering::Relaxed), 0);
    }
}
