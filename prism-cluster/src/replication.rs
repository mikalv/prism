//! Async primary→replica write replication
//!
//! After a primary node successfully indexes or deletes documents,
//! the `ReplicationManager` fans out the same operation to all replica
//! nodes asynchronously. This is fire-and-forget in Phase 1 — replica
//! failures are logged but do not block the primary response.

use crate::client::ClusterClient;
use crate::config::ReplicationConfig;
use crate::federation::QueryRouter;
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

/// Resolve which shard owns `id`, using the exact same hash function the
/// query router uses for placement: `hash_to_shard(id, total_shard_count)`
/// modulo total shards, then matched by `shard_number`.
///
/// Returns the owning shard only if `node_id` is that shard's primary.
/// A write routed to a shard owned by another primary is the other
/// primary's responsibility, so we skip it here rather than duplicate it.
///
/// This MUST agree with [`QueryRouter::route_by_id`].
pub(crate) fn shard_for_id<'a>(
    id: &str,
    all_shards: &'a [crate::placement::ShardAssignment],
    node_id: &str,
) -> Option<&'a crate::placement::ShardAssignment> {
    if all_shards.is_empty() {
        return None;
    }
    let idx = QueryRouter::hash_to_shard(id, all_shards.len());
    let owning = all_shards.iter().find(|s| s.shard_number as usize == idx)?;
    if owning.primary_node != node_id {
        return None;
    }
    Some(owning)
}

/// Group documents by owning shard (among shards `node_id` is primary for).
/// Returns `(shard_id, docs)` pairs ready for [`ReplicationManager::replicate_write`].
/// Documents owned by another primary are dropped (their primary replicates them).
pub(crate) fn group_docs_by_shards(
    cluster_state: &ClusterState,
    node_id: &str,
    collection: &str,
    docs: Vec<RpcDocument>,
) -> Vec<(String, Vec<RpcDocument>)> {
    let all = cluster_state.get_collection_shards(collection);
    if !all.iter().any(|s| s.primary_node == node_id) {
        return Vec::new();
    }
    let mut buckets: std::collections::HashMap<String, Vec<RpcDocument>> =
        std::collections::HashMap::new();
    for doc in docs {
        match shard_for_id(&doc.id, &all, node_id) {
            Some(shard) => buckets.entry(shard.shard_id.clone()).or_default().push(doc),
            None => debug!(
                "No primary shard for collection {} owns doc id {}; skipping",
                collection, doc.id
            ),
        }
    }
    buckets.into_iter().collect()
}

/// Group IDs by owning shard (among shards `node_id` is primary for).
/// Returns `(shard_id, ids)` pairs ready for [`ReplicationManager::replicate_write`].
pub(crate) fn group_ids_by_shards(
    cluster_state: &ClusterState,
    node_id: &str,
    collection: &str,
    ids: Vec<String>,
) -> Vec<(String, Vec<String>)> {
    let all = cluster_state.get_collection_shards(collection);
    if !all.iter().any(|s| s.primary_node == node_id) {
        return Vec::new();
    }
    let mut buckets: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for id in ids {
        match shard_for_id(&id, &all, node_id) {
            Some(shard) => buckets.entry(shard.shard_id.clone()).or_default().push(id),
            None => debug!(
                "No primary shard for collection {} owns id {}; skipping",
                collection, id
            ),
        }
    }
    buckets.into_iter().collect()
}

/// Collect `(node_id, address)` pairs for all reachable replicas of `shard_id`.
///
/// Returns an empty vec if the shard is unknown or has no replicas configured.
/// If replicas are configured but none are reachable, emits a `warn!` so the
/// "all unreachable" case is visible (rather than silently becoming a no-op).
pub(crate) fn replica_addresses(
    cluster_state: &ClusterState,
    shard_id: &str,
) -> Vec<(String, String)> {
    let shard = match cluster_state.get_shard(shard_id) {
        Some(s) => s,
        None => return Vec::new(),
    };

    if shard.replica_nodes.is_empty() {
        // No replicas configured for this shard; nothing to warn about.
        return Vec::new();
    }

    let mut addrs = Vec::new();
    let mut missing = 0;
    for replica_node_id in &shard.replica_nodes {
        if let Some(node_state) = cluster_state.get_node(replica_node_id) {
            addrs.push((replica_node_id.clone(), node_state.info.address.clone()));
        } else {
            missing += 1;
            warn!(
                "Replica node {} for shard {} not found in cluster state",
                replica_node_id, shard_id
            );
        }
    }

    // Explicit guard requested in review: if the shard *does* have replicas
    // configured but none of them are currently reachable, log loudly rather
    // than silently treating replication as a no-op. The caller
    // (replicate_write) still returns early on an empty list, but this makes
    // the "everything was unreachable" case visible.
    if addrs.is_empty() && missing > 0 {
        warn!(
            "No reachable replicas for shard {} ({} configured, {} unreachable)",
            shard_id,
            shard.replica_nodes.len(),
            missing
        );
    }
    addrs
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
    ///
    /// **Not used for write replication.** A single collection can have many
    /// shards, and this node can be the primary of more than one of them, so
    /// returning *the first* such shard is nondeterministic and could route a
    /// write to the wrong replica set. Use [`group_docs_by_shards`] /
    /// [`group_ids_by_shards`] instead, which derive the owning shard from
    /// each document/id via the same hash used for routing.
    ///
    /// Kept for callers that genuinely want "am I a primary for this
    /// collection at all" semantics.
    #[allow(dead_code)]
    pub fn primary_shard_for_collection(&self, collection: &str) -> Option<String> {
        let shards = self.cluster_state.get_collection_shards(collection);
        for shard in shards {
            if shard.primary_node == self.node_id {
                return Some(shard.shard_id);
            }
        }
        None
    }

    /// Group documents by the owning shard (among shards this node is primary
    /// for). Thin delegator over [`group_docs_by_shards`].
    pub fn group_docs_by_shards(
        &self,
        collection: &str,
        docs: Vec<RpcDocument>,
    ) -> Vec<(String, Vec<RpcDocument>)> {
        group_docs_by_shards(&self.cluster_state, &self.node_id, collection, docs)
    }

    /// Group IDs by the owning shard (among shards this node is primary for).
    /// Thin delegator over [`group_ids_by_shards`].
    pub fn group_ids_by_shards(
        &self,
        collection: &str,
        ids: Vec<String>,
    ) -> Vec<(String, Vec<String>)> {
        group_ids_by_shards(&self.cluster_state, &self.node_id, collection, ids)
    }

    /// Get the addresses of all replica nodes for the given shard.
    /// Thin delegator over [`replica_addresses`].
    fn replica_addresses(&self, shard_id: &str) -> Vec<(String, String)> {
        replica_addresses(&self.cluster_state, shard_id)
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
    use crate::placement::{ClusterState, ShardAssignment};

    /// Build a ClusterState with `n_shards` shards for `collection`, where
    /// shards with even `shard_number` have primary `primary_even` and odd
    /// ones have `primary_odd`. Each shard gets replicas in `replica_pool`.
    /// All nodes are registered so replica_addresses can resolve them.
    fn multi_shard_state(
        collection: &str,
        n_shards: u32,
        primary_even: &str,
        primary_odd: &str,
        replica_pool: &[&str],
    ) -> ClusterState {
        let state = ClusterState::new();
        // Register every node that appears anywhere.
        let mut nodes: Vec<String> = vec![primary_even.to_string(), primary_odd.to_string()];
        nodes.extend(replica_pool.iter().map(|s| s.to_string()));
        for n in &nodes {
            state.register_node(crate::placement::NodeInfo {
                node_id: n.clone(),
                address: format!("127.0.0.1:90{}", n.len()),
                topology: crate::config::NodeTopology::default(),
                healthy: true,
                shard_count: 0,
                disk_used_bytes: 0,
                disk_total_bytes: 100_000_000_000,
                index_size_bytes: 0,
                draining: false,
            });
        }
        for sn in 0..n_shards {
            let primary = if sn % 2 == 0 {
                primary_even
            } else {
                primary_odd
            };
            let mut a = ShardAssignment::new(collection, sn, primary);
            a.state = crate::ShardState::Active;
            a.replica_nodes = replica_pool.iter().map(|s| s.to_string()).collect();
            state.assign_shard(a);
        }
        state
    }

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

    #[test]
    fn shard_for_id_respects_primary_ownership() {
        // 4 shards: even -> node-a, odd -> node-b
        let state = multi_shard_state("c", 4, "node-a", "node-b", &["r1", "r2"]);
        let shards = state.get_collection_shards("c");
        assert_eq!(shards.len(), 4);

        // For any id, exactly one of node-a / node-b should be the owner,
        // never both, never neither.
        for id in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"] {
            let a = shard_for_id(id, &shards, "node-a");
            let b = shard_for_id(id, &shards, "node-b");
            assert!(
                a.is_some() ^ b.is_some(),
                "id {} should be owned by exactly one primary (a={:?}, b={:?})",
                id,
                a.map(|s| &s.shard_id),
                b.map(|s| &s.shard_id)
            );
        }
    }

    #[test]
    fn shard_for_id_matches_router_hash() {
        // The owning shard_number must equal what the router computes.
        // Both router and replication hash against total shard count, then
        // select the shard whose shard_number == that index. So resolve the
        // expected primary by shard_number, NOT by list position (HashMap
        // iteration order is unrelated to shard_number).
        let state = multi_shard_state("c", 4, "node-a", "node-b", &["r1"]);
        let shards = state.get_collection_shards("c");
        for id in ["alpha", "beta", "gamma", "0001", "ffff"] {
            let expected_idx = QueryRouter::hash_to_shard(id, shards.len());
            let expected_shard = shards
                .iter()
                .find(|s| s.shard_number as usize == expected_idx)
                .expect("shard with computed shard_number must exist");
            let owner = shard_for_id(id, &shards, &expected_shard.primary_node);
            assert_eq!(
                owner.map(|s| s.shard_number as usize),
                Some(expected_idx),
                "router/replication disagree on id {}",
                id
            );
        }
    }

    #[test]
    fn shard_for_id_empty_shards() {
        let state = ClusterState::new();
        let shards = state.get_collection_shards("c");
        assert!(shard_for_id("any", &shards, "node-a").is_none());
    }

    #[test]
    fn group_docs_routes_each_doc_to_owning_shard() {
        let state = multi_shard_state("c", 4, "node-a", "node-b", &["r1"]);
        let docs: Vec<RpcDocument> = (0..8)
            .map(|i| RpcDocument {
                id: format!("doc-{}", i),
                fields: Default::default(),
            })
            .collect();

        // node-a owns even shard_numbers (0, 2)
        let a_groups = group_docs_by_shards(&state, "node-a", "c", docs.clone());
        // node-b owns odd shard_numbers (1, 3)
        let b_groups = group_docs_by_shards(&state, "node-b", "c", docs.clone());

        let a_total: usize = a_groups.iter().map(|(_, d)| d.len()).sum();
        let b_total: usize = b_groups.iter().map(|(_, d)| d.len()).sum();
        assert_eq!(
            a_total + b_total,
            docs.len(),
            "every doc should land somewhere"
        );

        // No shard_id should appear in both primaries' groupings.
        let a_shards: std::collections::HashSet<_> =
            a_groups.iter().map(|(s, _)| s.clone()).collect();
        let b_shards: std::collections::HashSet<_> =
            b_groups.iter().map(|(s, _)| s.clone()).collect();
        assert!(
            a_shards.is_disjoint(&b_shards),
            "node-a and node-b must not replicate the same shard"
        );

        // And the shard ids we got back must all be shards that node actually owns.
        for sid in &a_shards {
            let sh = state.get_shard(sid).unwrap();
            assert_eq!(sh.primary_node, "node-a");
        }
        for sid in &b_shards {
            let sh = state.get_shard(sid).unwrap();
            assert_eq!(sh.primary_node, "node-b");
        }
    }

    #[test]
    fn group_ids_partitions_ids_correctly() {
        let state = multi_shard_state("c", 3, "node-a", "node-b", &["r1"]);
        let ids: Vec<String> = (0..9).map(|i| format!("id-{}", i)).collect();

        let a = group_ids_by_shards(&state, "node-a", "c", ids.clone());
        let b = group_ids_by_shards(&state, "node-b", "c", ids.clone());

        let a_total: usize = a.iter().map(|(_, v)| v.len()).sum();
        let b_total: usize = b.iter().map(|(_, v)| v.len()).sum();
        assert_eq!(a_total + b_total, ids.len());
    }

    #[test]
    fn grouping_no_primaries_returns_empty() {
        // node-c is primary for nothing.
        let state = multi_shard_state("c", 4, "node-a", "node-b", &["r1"]);
        assert!(group_docs_by_shards(
            &state,
            "node-c",
            "c",
            vec![RpcDocument {
                id: "x".into(),
                fields: Default::default(),
            }]
        )
        .is_empty());
        assert!(group_ids_by_shards(&state, "node-c", "c", vec!["x".to_string()]).is_empty());
    }

    #[test]
    fn replica_addresses_unknown_shard_is_empty() {
        let state = ClusterState::new();
        assert!(replica_addresses(&state, "nope").is_empty());
    }

    #[test]
    fn replica_addresses_no_replicas_configured_is_empty() {
        let state = ClusterState::new();
        state.register_node(crate::placement::NodeInfo {
            node_id: "n1".into(),
            address: "127.0.0.1:1".into(),
            topology: crate::config::NodeTopology::default(),
            healthy: true,
            shard_count: 0,
            disk_used_bytes: 0,
            disk_total_bytes: 0,
            index_size_bytes: 0,
            draining: false,
        });
        let mut a = ShardAssignment::new("c", 0, "n1");
        a.state = crate::ShardState::Active;
        // no replica_nodes set
        state.assign_shard(a);
        assert!(replica_addresses(&state, "c-shard-0").is_empty());
    }

    #[test]
    fn replica_addresses_resolves_all_configured_replicas() {
        let state = multi_shard_state("c", 2, "p", "p", &["r1", "r2"]);
        let addrs = replica_addresses(&state, "c-shard-0");
        // 2 replicas configured, both registered -> both resolved
        assert_eq!(addrs.len(), 2);
        let ids: Vec<_> = addrs.iter().map(|(n, _)| n.as_str()).collect();
        assert!(ids.contains(&"r1"));
        assert!(ids.contains(&"r2"));
    }

    #[test]
    fn replica_addresses_skips_unreachable_warns() {
        // Build state where the shard lists a replica node that is NOT
        // registered, so replica_addresses must drop it and log a warning.
        let state = ClusterState::new();
        state.register_node(crate::placement::NodeInfo {
            node_id: "p".into(),
            address: "127.0.0.1:1".into(),
            topology: crate::config::NodeTopology::default(),
            healthy: true,
            shard_count: 0,
            disk_used_bytes: 0,
            disk_total_bytes: 0,
            index_size_bytes: 0,
            draining: false,
        });
        // register r1 reachable, leave r2 unregistered
        state.register_node(crate::placement::NodeInfo {
            node_id: "r1".into(),
            address: "127.0.0.1:2".into(),
            topology: crate::config::NodeTopology::default(),
            healthy: true,
            shard_count: 0,
            disk_used_bytes: 0,
            disk_total_bytes: 0,
            index_size_bytes: 0,
            draining: false,
        });
        let mut a = ShardAssignment::new("c", 0, "p");
        a.state = crate::ShardState::Active;
        a.replica_nodes = vec!["r1".to_string(), "r2".to_string()]; // r2 missing
        state.assign_shard(a);

        let addrs = replica_addresses(&state, "c-shard-0");
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].0, "r1");
    }
}
