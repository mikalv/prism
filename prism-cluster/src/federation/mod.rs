//! Federation layer for distributed query execution
//!
//! Provides scatter-gather query execution across multiple Prism nodes
//! with configurable routing strategies and result merging.
//!
//! # Architecture
//!
//! ```text
//! Query → Router → [Shard 1, Shard 2, ...] → Merger → Results
//!                         ↓
//!              Parallel execution with timeout
//!                         ↓
//!              Partial results on failure
//! ```
//!
//! # Example
//!
//! ```ignore
//! use prism_cluster::federation::{FederatedSearch, FederationConfig};
//!
//! let federation = FederatedSearch::new(client, cluster_state, config);
//!
//! // Execute distributed search
//! let results = federation.search("products", query).await?;
//!
//! // Check shard status
//! if results.shard_status.failed > 0 {
//!     warn!("Some shards failed: {:?}", results.shard_status.failures);
//! }
//! ```

mod merger;
mod router;

pub use merger::{MergeStrategy, ResultMerger, ScoreNormalizer};
pub use router::{QueryRouter, RoutingDecision, RoutingStrategy, ShardTarget};

use crate::error::{ClusterError, Result};
use crate::placement::ClusterState;
use crate::types::{RpcDocument, RpcQuery, RpcSearchResult, RpcSearchResults};
use crate::ClusterClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tracing::{debug, warn};

/// Federation configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FederationConfig {
    /// Allow returning partial results when some shards fail
    #[serde(default = "default_allow_partial")]
    pub allow_partial_results: bool,

    /// Timeout for partial results (wait this long for slow shards)
    #[serde(default = "default_partial_timeout")]
    pub partial_results_timeout_ms: u64,

    /// Minimum number of successful shards required
    #[serde(default = "default_min_shards")]
    pub min_successful_shards: usize,

    /// Maximum concurrent shard requests
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,

    /// Default merge strategy
    #[serde(default)]
    pub default_merge_strategy: MergeStrategy,
}

fn default_allow_partial() -> bool {
    true
}

fn default_partial_timeout() -> u64 {
    5000
}

fn default_min_shards() -> usize {
    1
}

fn default_max_concurrent() -> usize {
    10
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            allow_partial_results: default_allow_partial(),
            partial_results_timeout_ms: default_partial_timeout(),
            min_successful_shards: default_min_shards(),
            max_concurrent_requests: default_max_concurrent(),
            default_merge_strategy: MergeStrategy::default(),
        }
    }
}

/// Status of shard execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardStatus {
    /// Total number of shards queried
    pub total: u32,
    /// Number of successful shards
    pub successful: u32,
    /// Number of failed shards
    pub failed: u32,
    /// Details of failures
    pub failures: Vec<ShardFailure>,
}

impl ShardStatus {
    /// Create a new shard status
    pub fn new(total: u32) -> Self {
        Self {
            total,
            successful: 0,
            failed: 0,
            failures: Vec::new(),
        }
    }

    /// Record a success
    pub fn record_success(&mut self) {
        self.successful += 1;
    }

    /// Record a failure
    pub fn record_failure(&mut self, failure: ShardFailure) {
        self.failed += 1;
        self.failures.push(failure);
    }

    /// Check if all shards succeeded
    pub fn all_succeeded(&self) -> bool {
        self.failed == 0
    }

    /// Check if at least min_shards succeeded
    pub fn has_minimum(&self, min_shards: usize) -> bool {
        self.successful as usize >= min_shards
    }
}

/// Details of a shard failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardFailure {
    /// Shard identifier
    pub shard_id: String,
    /// Node that was queried
    pub node: String,
    /// Error message
    pub reason: String,
    /// Whether this was a timeout
    pub is_timeout: bool,
}

/// Federated search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedResults {
    /// Merged search results
    pub results: Vec<RpcSearchResult>,
    /// Total hits across all shards
    pub total: usize,
    /// Total latency for the federated query
    pub latency_ms: u64,
    /// Shard execution status
    pub shard_status: ShardStatus,
    /// Whether results are partial
    pub is_partial: bool,
    /// Merge strategy used
    pub merge_strategy: MergeStrategy,
}

impl FederatedResults {
    /// Convert to RpcSearchResults (for API compatibility)
    pub fn to_rpc_results(&self) -> RpcSearchResults {
        RpcSearchResults {
            results: self.results.clone(),
            total: self.total,
            latency_ms: self.latency_ms,
        }
    }
}

/// Federated search executor
pub struct FederatedSearch {
    client: Arc<ClusterClient>,
    cluster_state: Arc<ClusterState>,
    config: FederationConfig,
    router: QueryRouter,
    merger: ResultMerger,
    semaphore: Arc<Semaphore>,
}

impl FederatedSearch {
    /// Create a new federated search executor
    pub fn new(
        client: Arc<ClusterClient>,
        cluster_state: Arc<ClusterState>,
        config: FederationConfig,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_requests));
        let router = QueryRouter::new(Arc::clone(&cluster_state));
        let merger = ResultMerger::new(config.default_merge_strategy.clone());

        Self {
            client,
            cluster_state,
            config,
            router,
            merger,
            semaphore,
        }
    }

    /// Execute a federated search across all relevant shards
    pub async fn search(&self, collection: &str, query: RpcQuery) -> Result<FederatedResults> {
        let start = Instant::now();

        // Get routing decision
        let routing = self.router.route(collection, &query)?;
        debug!(
            "Routing query to {} shards: {:?}",
            routing.targets.len(),
            routing
                .targets
                .iter()
                .map(|t| &t.shard_id)
                .collect::<Vec<_>>()
        );

        if routing.targets.is_empty() {
            return Ok(FederatedResults {
                results: Vec::new(),
                total: 0,
                latency_ms: start.elapsed().as_millis() as u64,
                shard_status: ShardStatus::new(0),
                is_partial: false,
                merge_strategy: self.config.default_merge_strategy.clone(),
            });
        }

        // Execute scatter-gather
        let (shard_results, shard_status) = self
            .scatter_gather(collection, &query, &routing.targets)
            .await;

        // Check if we have enough results
        if !shard_status.has_minimum(self.config.min_successful_shards) {
            return Err(ClusterError::Internal(format!(
                "Insufficient shards: {} successful, {} required",
                shard_status.successful, self.config.min_successful_shards
            )));
        }

        // Merge results
        let merge_strategy = query
            .merge_strategy
            .as_ref()
            .and_then(|s| MergeStrategy::from_string(s))
            .unwrap_or_else(|| self.config.default_merge_strategy.clone());

        let merged = self
            .merger
            .merge(shard_results, query.limit, &merge_strategy);

        let latency_ms = start.elapsed().as_millis() as u64;
        let is_partial = shard_status.failed > 0;

        if is_partial {
            warn!(
                "Federated search returned partial results: {}/{} shards failed",
                shard_status.failed, shard_status.total
            );
        }

        Ok(FederatedResults {
            results: merged.results,
            total: merged.total,
            latency_ms,
            shard_status,
            is_partial,
            merge_strategy,
        })
    }

    /// Scatter query to shards and gather results
    async fn scatter_gather(
        &self,
        collection: &str,
        query: &RpcQuery,
        targets: &[ShardTarget],
    ) -> (Vec<RpcSearchResults>, ShardStatus) {
        let mut shard_status = ShardStatus::new(targets.len() as u32);
        let timeout = Duration::from_millis(self.config.partial_results_timeout_ms);

        // Create futures for each shard
        let futures: Vec<_> = targets
            .iter()
            .map(|target| {
                let client = Arc::clone(&self.client);
                let semaphore = Arc::clone(&self.semaphore);
                let collection = collection.to_string();
                let query = query.clone();
                let shard_id = target.shard_id.clone();
                let node_addr = target.node_address.clone();

                async move {
                    // Acquire semaphore permit
                    let _permit = semaphore.acquire().await.ok();

                    let result = client.search(&node_addr, &collection, query).await;
                    (shard_id, node_addr, result)
                }
            })
            .collect();

        // Execute with timeout
        let results = if self.config.allow_partial_results {
            // Use timeout for partial results
            tokio::time::timeout(timeout, async { futures::future::join_all(futures).await })
                .await
                .unwrap_or_else(|_| {
                    // Timeout - return empty results for timed-out shards
                    warn!("Federated search timed out after {}ms", timeout.as_millis());
                    Vec::new()
                })
        } else {
            // Wait for all results
            futures::future::join_all(futures).await
        };

        // Process results
        let mut shard_results = Vec::new();

        for (shard_id, node_addr, result) in results {
            match result {
                Ok(search_results) => {
                    debug!(
                        "Shard {} ({}) returned {} results (total={})",
                        shard_id,
                        node_addr,
                        search_results.results.len(),
                        search_results.total
                    );
                    shard_status.record_success();
                    shard_results.push(search_results);
                }
                Err(e) => {
                    warn!("Shard {} ({}) failed: {}", shard_id, node_addr, e);
                    let is_timeout = matches!(e, ClusterError::Timeout(_));
                    shard_status.record_failure(ShardFailure {
                        shard_id,
                        node: node_addr,
                        reason: e.to_string(),
                        is_timeout,
                    });
                }
            }
        }

        (shard_results, shard_status)
    }

    /// Execute a federated get (retrieve document by ID from any shard)
    pub async fn get(&self, collection: &str, id: &str) -> Result<Option<RpcDocument>> {
        // Route to the shard that should contain this document
        let routing = self.router.route_by_id(collection, id)?;

        for target in &routing.targets {
            match self.client.get(&target.node_address, collection, id).await {
                Ok(Some(doc)) => return Ok(Some(doc)),
                Ok(None) => continue, // Try next replica
                Err(e) => {
                    debug!(
                        "Get from {} failed: {}, trying next",
                        target.node_address, e
                    );
                    continue;
                }
            }
        }

        Ok(None)
    }

    /// Execute a federated index (route documents to correct shards)
    pub async fn index(&self, collection: &str, docs: Vec<RpcDocument>) -> Result<IndexStatus> {
        let start = Instant::now();

        // Group documents by target shard
        let mut shard_docs: HashMap<String, Vec<RpcDocument>> = HashMap::new();

        for doc in docs {
            let routing = self.router.route_by_id(collection, &doc.id)?;
            if let Some(target) = routing.targets.first() {
                shard_docs
                    .entry(target.node_address.clone())
                    .or_default()
                    .push(doc);
            }
        }

        // Index to each shard
        let mut status = IndexStatus {
            total_docs: 0,
            successful_docs: 0,
            failed_docs: 0,
            latency_ms: 0,
            shard_status: ShardStatus::new(shard_docs.len() as u32),
        };

        let futures: Vec<_> = shard_docs
            .into_iter()
            .map(|(addr, docs)| {
                let client = Arc::clone(&self.client);
                let collection = collection.to_string();
                let doc_count = docs.len();

                async move {
                    let result = client.index(&addr, &collection, docs).await;
                    (addr, doc_count, result)
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        for (addr, doc_count, result) in results {
            status.total_docs += doc_count;
            match result {
                Ok(()) => {
                    status.successful_docs += doc_count;
                    status.shard_status.record_success();
                }
                Err(e) => {
                    status.failed_docs += doc_count;
                    status.shard_status.record_failure(ShardFailure {
                        shard_id: addr.clone(),
                        node: addr,
                        reason: e.to_string(),
                        is_timeout: false,
                    });
                }
            }
        }

        status.latency_ms = start.elapsed().as_millis() as u64;
        Ok(status)
    }

    /// Execute a federated delete
    pub async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<DeleteStatus> {
        let start = Instant::now();

        // Group IDs by target shard
        let mut shard_ids: HashMap<String, Vec<String>> = HashMap::new();

        for id in ids {
            let routing = self.router.route_by_id(collection, &id)?;
            if let Some(target) = routing.targets.first() {
                shard_ids
                    .entry(target.node_address.clone())
                    .or_default()
                    .push(id);
            }
        }

        // Delete from each shard
        let mut status = DeleteStatus {
            total_ids: 0,
            successful_deletes: 0,
            failed_deletes: 0,
            latency_ms: 0,
            shard_status: ShardStatus::new(shard_ids.len() as u32),
        };

        let futures: Vec<_> = shard_ids
            .into_iter()
            .map(|(addr, ids)| {
                let client = Arc::clone(&self.client);
                let collection = collection.to_string();
                let id_count = ids.len();

                async move {
                    let result = client.delete(&addr, &collection, ids).await;
                    (addr, id_count, result)
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        for (addr, id_count, result) in results {
            status.total_ids += id_count;
            match result {
                Ok(()) => {
                    status.successful_deletes += id_count;
                    status.shard_status.record_success();
                }
                Err(e) => {
                    status.failed_deletes += id_count;
                    status.shard_status.record_failure(ShardFailure {
                        shard_id: addr.clone(),
                        node: addr,
                        reason: e.to_string(),
                        is_timeout: false,
                    });
                }
            }
        }

        status.latency_ms = start.elapsed().as_millis() as u64;
        Ok(status)
    }

    /// Get aggregated stats across all shards
    pub async fn stats(&self, collection: &str) -> Result<AggregatedStats> {
        let shards = self.cluster_state.get_collection_shards(collection);

        if shards.is_empty() {
            return Err(ClusterError::CollectionNotFound(collection.to_string()));
        }

        let mut stats = AggregatedStats {
            total_documents: 0,
            total_size_bytes: 0,
            shard_count: shards.len(),
            shard_stats: Vec::new(),
        };

        for shard in shards {
            if let Ok(shard_stats) = self.client.stats(&shard.primary_node, collection).await {
                stats.total_documents += shard_stats.document_count;
                stats.total_size_bytes += shard_stats.size_bytes;
                stats.shard_stats.push(ShardStats {
                    shard_id: shard.shard_id.clone(),
                    document_count: shard_stats.document_count,
                    size_bytes: shard_stats.size_bytes,
                });
            }
        }

        Ok(stats)
    }
}

/// Status of a federated index operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatus {
    pub total_docs: usize,
    pub successful_docs: usize,
    pub failed_docs: usize,
    pub latency_ms: u64,
    pub shard_status: ShardStatus,
}

/// Status of a federated delete operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteStatus {
    pub total_ids: usize,
    pub successful_deletes: usize,
    pub failed_deletes: usize,
    pub latency_ms: u64,
    pub shard_status: ShardStatus,
}

/// Aggregated stats across shards
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedStats {
    pub total_documents: usize,
    pub total_size_bytes: usize,
    pub shard_count: usize,
    pub shard_stats: Vec<ShardStats>,
}

/// Stats for a single shard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardStats {
    pub shard_id: String,
    pub document_count: usize,
    pub size_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_federation_config_default() {
        let config = FederationConfig::default();
        assert!(config.allow_partial_results);
        assert_eq!(config.partial_results_timeout_ms, 5000);
        assert_eq!(config.min_successful_shards, 1);
        assert_eq!(config.max_concurrent_requests, 10);
    }

    #[test]
    fn test_federation_config_serde_roundtrip() {
        let config = FederationConfig {
            allow_partial_results: false,
            partial_results_timeout_ms: 10000,
            min_successful_shards: 3,
            max_concurrent_requests: 20,
            default_merge_strategy: MergeStrategy::ScoreNormalized,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: FederationConfig = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.allow_partial_results);
        assert_eq!(deserialized.partial_results_timeout_ms, 10000);
        assert_eq!(deserialized.min_successful_shards, 3);
        assert_eq!(deserialized.max_concurrent_requests, 20);
    }

    #[test]
    fn test_federation_config_serde_defaults() {
        let config: FederationConfig = serde_json::from_str("{}").unwrap();
        assert!(config.allow_partial_results);
        assert_eq!(config.partial_results_timeout_ms, 5000);
    }

    // --- ShardStatus ---

    #[test]
    fn test_shard_status() {
        let mut status = ShardStatus::new(3);
        assert_eq!(status.total, 3);
        assert_eq!(status.successful, 0);
        assert_eq!(status.failed, 0);

        status.record_success();
        status.record_success();
        status.record_failure(ShardFailure {
            shard_id: "shard-1".into(),
            node: "node-1:9080".into(),
            reason: "timeout".into(),
            is_timeout: true,
        });

        assert_eq!(status.successful, 2);
        assert_eq!(status.failed, 1);
        assert!(status.has_minimum(2));
        assert!(!status.all_succeeded());
    }

    #[test]
    fn test_shard_status_all_succeeded() {
        let mut status = ShardStatus::new(2);
        status.record_success();
        status.record_success();

        assert!(status.all_succeeded());
        assert!(status.has_minimum(1));
        assert!(status.has_minimum(2));
        assert!(!status.has_minimum(3));
    }

    #[test]
    fn test_shard_status_all_failed() {
        let mut status = ShardStatus::new(2);
        status.record_failure(ShardFailure {
            shard_id: "s1".into(),
            node: "n1".into(),
            reason: "err".into(),
            is_timeout: false,
        });
        status.record_failure(ShardFailure {
            shard_id: "s2".into(),
            node: "n2".into(),
            reason: "err".into(),
            is_timeout: true,
        });

        assert!(!status.all_succeeded());
        assert!(!status.has_minimum(1));
        assert_eq!(status.failures.len(), 2);
        assert!(status.failures[0].shard_id == "s1");
        assert!(status.failures[1].is_timeout);
    }

    #[test]
    fn test_shard_status_empty() {
        let status = ShardStatus::new(0);
        assert!(status.all_succeeded());
        assert!(status.has_minimum(0));
        assert!(!status.has_minimum(1));
    }

    // --- ShardFailure ---

    #[test]
    fn test_shard_failure_fields() {
        let failure = ShardFailure {
            shard_id: "test-shard-0".into(),
            node: "node-1:9080".into(),
            reason: "Connection refused".into(),
            is_timeout: false,
        };
        assert_eq!(failure.shard_id, "test-shard-0");
        assert!(!failure.is_timeout);
    }

    #[test]
    fn test_shard_failure_serde() {
        let failure = ShardFailure {
            shard_id: "s1".into(),
            node: "n1".into(),
            reason: "timeout".into(),
            is_timeout: true,
        };
        let json = serde_json::to_string(&failure).unwrap();
        let deserialized: ShardFailure = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.shard_id, "s1");
        assert!(deserialized.is_timeout);
    }

    // --- FederatedResults ---

    #[test]
    fn test_federated_results_to_rpc_results() {
        let result = FederatedResults {
            results: vec![RpcSearchResult {
                id: "doc-1".into(),
                score: 0.95,
                fields: std::collections::HashMap::new(),
                highlight: None,
            }],
            total: 1,
            latency_ms: 50,
            shard_status: ShardStatus::new(1),
            is_partial: false,
            merge_strategy: MergeStrategy::Simple,
        };

        let rpc = result.to_rpc_results();
        assert_eq!(rpc.results.len(), 1);
        assert_eq!(rpc.total, 1);
        assert_eq!(rpc.latency_ms, 50);
    }

    #[test]
    fn test_federated_results_empty() {
        let result = FederatedResults {
            results: vec![],
            total: 0,
            latency_ms: 0,
            shard_status: ShardStatus::new(0),
            is_partial: false,
            merge_strategy: MergeStrategy::Simple,
        };

        let rpc = result.to_rpc_results();
        assert!(rpc.results.is_empty());
        assert_eq!(rpc.total, 0);
    }

    // --- IndexStatus / DeleteStatus ---

    #[test]
    fn test_index_status_serde() {
        let status = IndexStatus {
            total_docs: 100,
            successful_docs: 95,
            failed_docs: 5,
            latency_ms: 200,
            shard_status: ShardStatus::new(3),
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: IndexStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_docs, 100);
        assert_eq!(deserialized.failed_docs, 5);
    }

    #[test]
    fn test_delete_status_serde() {
        let status = DeleteStatus {
            total_ids: 50,
            successful_deletes: 48,
            failed_deletes: 2,
            latency_ms: 150,
            shard_status: ShardStatus::new(2),
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: DeleteStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_ids, 50);
        assert_eq!(deserialized.successful_deletes, 48);
    }

    // --- AggregatedStats ---

    #[test]
    fn test_aggregated_stats_serde() {
        let stats = AggregatedStats {
            total_documents: 1000,
            total_size_bytes: 1024 * 1024,
            shard_count: 3,
            shard_stats: vec![
                ShardStats {
                    shard_id: "s1".into(),
                    document_count: 500,
                    size_bytes: 512 * 1024,
                },
                ShardStats {
                    shard_id: "s2".into(),
                    document_count: 500,
                    size_bytes: 512 * 1024,
                },
            ],
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: AggregatedStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_documents, 1000);
        assert_eq!(deserialized.shard_stats.len(), 2);
    }
}
