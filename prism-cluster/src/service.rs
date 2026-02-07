//! tarpc service definition for cluster RPC
//!
//! Defines the RPC interface for inter-node communication, mirroring
//! the CollectionManager operations plus cluster-specific functionality.

use crate::error::ClusterError;
use crate::types::*;

/// Prism cluster RPC service definition.
///
/// This service provides the same core operations as CollectionManager
/// plus additional cluster-specific operations for bulk deletion and
/// cross-cluster data migration.
#[tarpc::service]
pub trait PrismCluster {
    // ========================================
    // Core Operations (mirror CollectionManager)
    // ========================================

    /// Index documents into a collection
    async fn index(collection: String, docs: Vec<RpcDocument>) -> Result<(), ClusterError>;

    /// Search documents in a collection
    async fn search(collection: String, query: RpcQuery) -> Result<RpcSearchResults, ClusterError>;

    /// Get a document by ID
    async fn get(collection: String, id: String) -> Result<Option<RpcDocument>, ClusterError>;

    /// Delete documents by IDs
    async fn delete(collection: String, ids: Vec<String>) -> Result<(), ClusterError>;

    /// Get collection statistics
    async fn stats(collection: String) -> Result<RpcBackendStats, ClusterError>;

    /// List all collections
    async fn list_collections() -> Vec<String>;

    // ========================================
    // Cluster-specific Operations
    // ========================================

    /// Delete documents matching a query
    ///
    /// Useful for bulk cleanup operations across the cluster.
    async fn delete_by_query(
        request: DeleteByQueryRequest,
    ) -> Result<DeleteByQueryResponse, ClusterError>;

    /// Import documents from a query (for cross-cluster migration)
    ///
    /// When source_node is specified, fetches documents from that node
    /// and indexes them into the target collection on this node.
    async fn import_by_query(
        request: ImportByQueryRequest,
    ) -> Result<ImportByQueryResponse, ClusterError>;

    // ========================================
    // Health & Discovery
    // ========================================

    /// Get node information
    async fn node_info() -> NodeInfo;

    /// Simple ping for health checking
    async fn ping() -> String;

    /// Get cluster health summary
    async fn cluster_health() -> RpcClusterHealth;

    /// Report heartbeat (for remote health checking)
    async fn heartbeat() -> RpcHeartbeatResponse;

    // ========================================
    // Shard Management
    // ========================================

    /// Assign a shard to nodes
    ///
    /// Creates or updates a shard assignment, specifying primary and replica nodes.
    async fn assign_shard(
        request: ShardAssignmentRequest,
    ) -> Result<ShardAssignmentResponse, ClusterError>;

    /// Get shard assignments for a collection
    ///
    /// Returns all shard assignments, optionally filtered by collection.
    async fn get_shard_assignments(
        request: GetShardAssignmentsRequest,
    ) -> Result<Vec<RpcShardInfo>, ClusterError>;

    /// Transfer a shard between nodes
    ///
    /// Initiates a shard transfer from source to target node.
    /// The transfer happens asynchronously; use get_rebalance_status to track.
    async fn transfer_shard(
        request: ShardTransferRequest,
    ) -> Result<ShardTransferResponse, ClusterError>;

    // ========================================
    // Rebalancing
    // ========================================

    /// Trigger cluster rebalancing
    ///
    /// Initiates rebalancing to redistribute shards evenly across nodes.
    async fn trigger_rebalance(
        request: TriggerRebalanceRequest,
    ) -> Result<RpcRebalanceStatus, ClusterError>;

    /// Get current rebalancing status
    async fn get_rebalance_status() -> Result<RpcRebalanceStatus, ClusterError>;
}
