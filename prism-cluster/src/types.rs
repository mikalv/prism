//! RPC types for cluster communication
//!
//! These types are serializable wrappers around core Prism types,
//! designed for efficient bincode serialization over the wire.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Serializable query type for RPC transport.
/// Mirrors prism::backends::Query but with Serialize/Deserialize derives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcQuery {
    pub query_string: String,
    pub fields: Vec<String>,
    pub limit: usize,
    pub offset: usize,
    pub merge_strategy: Option<String>,
    pub text_weight: Option<f32>,
    pub vector_weight: Option<f32>,
    pub highlight: Option<RpcHighlightConfig>,
    #[serde(default)]
    pub rrf_k: Option<usize>,
    #[serde(default)]
    pub min_score: Option<f32>,
    #[serde(default)]
    pub score_function: Option<String>,
    #[serde(default)]
    pub skip_ranking: bool,
}

impl From<prism::backends::Query> for RpcQuery {
    fn from(q: prism::backends::Query) -> Self {
        Self {
            query_string: q.query_string,
            fields: q.fields,
            limit: q.limit,
            offset: q.offset,
            merge_strategy: q.merge_strategy,
            text_weight: q.text_weight,
            vector_weight: q.vector_weight,
            highlight: q.highlight.map(RpcHighlightConfig::from),
            rrf_k: q.rrf_k,
            min_score: q.min_score,
            score_function: q.score_function,
            skip_ranking: q.skip_ranking,
        }
    }
}

impl From<RpcQuery> for prism::backends::Query {
    fn from(q: RpcQuery) -> Self {
        Self {
            query_string: q.query_string,
            fields: q.fields,
            limit: q.limit,
            offset: q.offset,
            merge_strategy: q.merge_strategy,
            text_weight: q.text_weight,
            vector_weight: q.vector_weight,
            highlight: q.highlight.map(prism::backends::HighlightConfig::from),
            rrf_k: q.rrf_k,
            min_score: q.min_score,
            score_function: q.score_function,
            skip_ranking: q.skip_ranking,
        }
    }
}

/// Serializable highlight configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcHighlightConfig {
    pub fields: Vec<String>,
    pub pre_tag: String,
    pub post_tag: String,
    pub fragment_size: usize,
    pub number_of_fragments: usize,
}

impl From<prism::backends::HighlightConfig> for RpcHighlightConfig {
    fn from(h: prism::backends::HighlightConfig) -> Self {
        Self {
            fields: h.fields,
            pre_tag: h.pre_tag,
            post_tag: h.post_tag,
            fragment_size: h.fragment_size,
            number_of_fragments: h.number_of_fragments,
        }
    }
}

impl From<RpcHighlightConfig> for prism::backends::HighlightConfig {
    fn from(h: RpcHighlightConfig) -> Self {
        Self {
            fields: h.fields,
            pre_tag: h.pre_tag,
            post_tag: h.post_tag,
            fragment_size: h.fragment_size,
            number_of_fragments: h.number_of_fragments,
        }
    }
}

/// RPC document wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcDocument {
    pub id: String,
    pub fields: HashMap<String, Value>,
}

impl From<prism::backends::Document> for RpcDocument {
    fn from(d: prism::backends::Document) -> Self {
        Self {
            id: d.id,
            fields: d.fields,
        }
    }
}

impl From<RpcDocument> for prism::backends::Document {
    fn from(d: RpcDocument) -> Self {
        Self {
            id: d.id,
            fields: d.fields,
        }
    }
}

/// RPC search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcSearchResult {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, Value>,
    pub highlight: Option<HashMap<String, Vec<String>>>,
}

impl From<prism::backends::SearchResult> for RpcSearchResult {
    fn from(r: prism::backends::SearchResult) -> Self {
        Self {
            id: r.id,
            score: r.score,
            fields: r.fields,
            highlight: r.highlight,
        }
    }
}

impl From<RpcSearchResult> for prism::backends::SearchResult {
    fn from(r: RpcSearchResult) -> Self {
        Self {
            id: r.id,
            score: r.score,
            fields: r.fields,
            highlight: r.highlight,
        }
    }
}

/// RPC search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcSearchResults {
    pub results: Vec<RpcSearchResult>,
    pub total: usize,
    pub latency_ms: u64,
}

impl From<prism::backends::SearchResults> for RpcSearchResults {
    fn from(r: prism::backends::SearchResults) -> Self {
        Self {
            results: r.results.into_iter().map(RpcSearchResult::from).collect(),
            total: r.total,
            latency_ms: r.latency_ms,
        }
    }
}

impl From<RpcSearchResults> for prism::backends::SearchResults {
    fn from(r: RpcSearchResults) -> Self {
        Self {
            results: r
                .results
                .into_iter()
                .map(prism::backends::SearchResult::from)
                .collect(),
            total: r.total,
            latency_ms: r.latency_ms,
        }
    }
}

/// RPC backend stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcBackendStats {
    pub document_count: usize,
    pub size_bytes: usize,
}

impl From<prism::backends::BackendStats> for RpcBackendStats {
    fn from(s: prism::backends::BackendStats) -> Self {
        Self {
            document_count: s.document_count,
            size_bytes: s.size_bytes,
        }
    }
}

impl From<RpcBackendStats> for prism::backends::BackendStats {
    fn from(s: RpcBackendStats) -> Self {
        Self {
            document_count: s.document_count,
            size_bytes: s.size_bytes,
        }
    }
}

/// Request for deleting documents by query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteByQueryRequest {
    /// Collection to delete from
    pub collection: String,
    /// Query to match documents for deletion
    pub query: RpcQuery,
    /// Maximum number of documents to delete (0 = unlimited)
    pub max_docs: usize,
    /// If true, only return count without deleting
    pub dry_run: bool,
}

/// Response from delete-by-query operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteByQueryResponse {
    /// Number of documents deleted (or would be deleted in dry_run)
    pub deleted_count: usize,
    /// Time taken in milliseconds
    pub took_ms: u64,
    /// IDs of deleted documents (only populated if request had max_docs > 0)
    pub deleted_ids: Vec<String>,
}

/// Request for importing documents from another cluster via query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportByQueryRequest {
    /// Source collection to query
    pub source_collection: String,
    /// Target collection to import into
    pub target_collection: String,
    /// Query to select documents for import
    pub query: RpcQuery,
    /// Source node address (if importing from remote)
    pub source_node: Option<String>,
    /// Batch size for streaming documents
    pub batch_size: usize,
}

/// Response from import-by-query operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportByQueryResponse {
    /// Number of documents imported
    pub imported_count: usize,
    /// Number of documents that failed to import
    pub failed_count: usize,
    /// Time taken in milliseconds
    pub took_ms: u64,
    /// Error messages for failed documents
    pub errors: Vec<String>,
}

/// Information about a cluster node (for RPC responses)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique node identifier
    pub node_id: String,
    /// Node version
    pub version: String,
    /// List of collections on this node
    pub collections: Vec<String>,
    /// Uptime in seconds
    pub uptime_secs: u64,
    /// Whether the node is healthy
    pub healthy: bool,
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

// ========================================
// Shard Management Types
// ========================================

/// Request to assign a shard to nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardAssignmentRequest {
    /// Shard ID
    pub shard_id: String,
    /// Collection name
    pub collection: String,
    /// Primary node ID
    pub primary_node: String,
    /// Replica node IDs
    pub replica_nodes: Vec<String>,
    /// Shard number
    pub shard_number: u32,
}

/// Response from shard assignment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardAssignmentResponse {
    /// Whether assignment was successful
    pub success: bool,
    /// Current epoch after assignment
    pub epoch: u64,
    /// Error message if failed
    pub error: Option<String>,
}

/// Request to get shard assignments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetShardAssignmentsRequest {
    /// Collection name (None = all collections)
    pub collection: Option<String>,
}

/// Shard info for RPC transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcShardInfo {
    /// Shard ID
    pub shard_id: String,
    /// Collection name
    pub collection: String,
    /// Primary node ID
    pub primary_node: String,
    /// Replica node IDs
    pub replica_nodes: Vec<String>,
    /// Shard state
    pub state: String,
    /// Shard number
    pub shard_number: u32,
    /// Size in bytes
    pub size_bytes: u64,
    /// Document count
    pub document_count: u64,
}

/// Request to transfer a shard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardTransferRequest {
    /// Shard ID to transfer
    pub shard_id: String,
    /// Source node
    pub from_node: String,
    /// Target node
    pub to_node: String,
    /// Whether this is a rebalance (vs recovery)
    pub is_rebalance: bool,
}

/// Response from shard transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardTransferResponse {
    /// Whether transfer was initiated
    pub success: bool,
    /// Transfer ID for tracking
    pub transfer_id: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Request to trigger rebalancing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerRebalanceRequest {
    /// Collection to rebalance (None = all)
    pub collection: Option<String>,
    /// Trigger reason
    pub trigger: String,
}

/// Status of rebalancing for RPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRebalanceStatus {
    /// Whether rebalancing is in progress
    pub in_progress: bool,
    /// Current phase
    pub phase: String,
    /// Shards currently being moved
    pub shards_in_transit: usize,
    /// Total shards to move
    pub total_shards_to_move: usize,
    /// Completed moves
    pub completed_moves: usize,
    /// Failed moves
    pub failed_moves: usize,
    /// Start time (Unix epoch)
    pub started_at: Option<u64>,
    /// Last error
    pub last_error: Option<String>,
}

// ================================
// Health Check Types
// ================================

/// Health state of a node for RPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNodeHealth {
    /// Node ID
    pub node_id: String,
    /// Health state: alive, suspect, dead
    pub state: String,
    /// Last heartbeat timestamp (Unix epoch seconds)
    pub last_heartbeat: Option<u64>,
    /// Consecutive missed heartbeats
    pub missed_heartbeats: u32,
    /// Last heartbeat latency in milliseconds
    pub last_latency_ms: Option<u64>,
    /// Whether this node is draining
    #[serde(default)]
    pub draining: bool,
}

/// Cluster health summary for RPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcClusterHealth {
    /// Health state of each node
    pub nodes: Vec<RpcNodeHealth>,
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

/// Response to heartbeat request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcHeartbeatResponse {
    /// Node ID responding
    pub node_id: String,
    /// Node version
    pub version: String,
    /// Server uptime in seconds
    pub uptime_secs: u64,
    /// Timestamp of response (Unix epoch seconds)
    pub timestamp: u64,
    /// Protocol version this node speaks
    #[serde(default)]
    pub protocol_version: u32,
    /// Minimum protocol version this node supports
    #[serde(default)]
    pub min_supported_version: u32,
}

// ================================
// Schema Propagation Types
// ================================

/// Request to apply a schema from another node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcApplySchemaRequest {
    /// Collection name
    pub collection: String,
    /// Schema version number
    pub version: u64,
    /// Schema content (JSON)
    pub schema: Value,
    /// When this version was created (unix timestamp ms)
    pub created_at: u64,
    /// Node that created this version
    pub created_by: String,
    /// Changes from previous version
    pub changes: Vec<RpcSchemaChange>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// A schema change description for RPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcSchemaChange {
    /// Type of change (field_added, field_removed, etc.)
    pub change_type: String,
    /// Path to the changed element
    pub path: String,
    /// Previous value
    pub old_value: Option<Value>,
    /// New value
    pub new_value: Option<Value>,
    /// Human-readable description
    pub description: String,
}

/// Response from applying a schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcApplySchemaResponse {
    /// Whether the schema was applied
    pub applied: bool,
    /// Current schema version after operation
    pub current_version: u64,
    /// Error message if failed
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_backward_compat_heartbeat() {
        // Old-format heartbeat (no protocol_version fields) should deserialize with defaults
        let json = r#"{
            "node_id": "node-1",
            "version": "0.6.0",
            "uptime_secs": 100,
            "timestamp": 1700000000
        }"#;

        let response: RpcHeartbeatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.node_id, "node-1");
        assert_eq!(response.protocol_version, 0);
        assert_eq!(response.min_supported_version, 0);
    }

    #[test]
    fn test_serde_backward_compat_node_info() {
        // Old-format NodeInfo (no draining/version fields)
        let json = r#"{
            "node_id": "node-1",
            "version": "0.6.0",
            "collections": ["test"],
            "uptime_secs": 100,
            "healthy": true
        }"#;

        let info: NodeInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.node_id, "node-1");
        assert_eq!(info.protocol_version, 0);
        assert_eq!(info.min_supported_version, 0);
        assert!(!info.draining);
    }

    #[test]
    fn test_serde_backward_compat_node_health() {
        // Old-format RpcNodeHealth (no draining field)
        let json = r#"{
            "node_id": "node-1",
            "state": "alive",
            "last_heartbeat": 1700000000,
            "missed_heartbeats": 0,
            "last_latency_ms": 5
        }"#;

        let health: RpcNodeHealth = serde_json::from_str(json).unwrap();
        assert_eq!(health.node_id, "node-1");
        assert!(!health.draining);
    }

    #[test]
    fn test_serde_new_format_heartbeat() {
        // New-format with protocol version fields
        let json = r#"{
            "node_id": "node-2",
            "version": "0.7.0",
            "uptime_secs": 200,
            "timestamp": 1700000000,
            "protocol_version": 2,
            "min_supported_version": 1
        }"#;

        let response: RpcHeartbeatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.protocol_version, 2);
        assert_eq!(response.min_supported_version, 1);
    }

    // --- From/Into round-trip tests ---

    #[test]
    fn test_rpc_query_roundtrip() {
        let query = prism::backends::Query {
            query_string: "test search".into(),
            fields: vec!["title".into(), "body".into()],
            limit: 10,
            offset: 5,
            merge_strategy: Some("rrf".into()),
            text_weight: Some(0.7),
            vector_weight: Some(0.3),
            highlight: Some(prism::backends::HighlightConfig {
                fields: vec!["title".into()],
                pre_tag: "<em>".into(),
                post_tag: "</em>".into(),
                fragment_size: 100,
                number_of_fragments: 3,
            }),
            rrf_k: Some(60),
            min_score: Some(0.5),
            score_function: Some("bm25".into()),
            skip_ranking: true,
        };

        let rpc: RpcQuery = query.clone().into();
        let back: prism::backends::Query = rpc.into();

        assert_eq!(back.query_string, query.query_string);
        assert_eq!(back.fields, query.fields);
        assert_eq!(back.limit, query.limit);
        assert_eq!(back.offset, query.offset);
        assert_eq!(back.merge_strategy, query.merge_strategy);
        assert_eq!(back.text_weight, query.text_weight);
        assert_eq!(back.vector_weight, query.vector_weight);
        assert_eq!(back.rrf_k, query.rrf_k);
        assert_eq!(back.min_score, query.min_score);
        assert_eq!(back.score_function, query.score_function);
        assert_eq!(back.skip_ranking, query.skip_ranking);
        assert!(back.highlight.is_some());
    }

    #[test]
    fn test_rpc_query_no_highlight() {
        let query = prism::backends::Query {
            query_string: "simple".into(),
            fields: vec![],
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
            rrf_k: None,
            min_score: None,
            score_function: None,
            skip_ranking: false,
        };

        let rpc: RpcQuery = query.into();
        let back: prism::backends::Query = rpc.into();
        assert!(back.highlight.is_none());
        assert!(!back.skip_ranking);
    }

    #[test]
    fn test_rpc_highlight_config_roundtrip() {
        let config = prism::backends::HighlightConfig {
            fields: vec!["title".into(), "body".into()],
            pre_tag: "<b>".into(),
            post_tag: "</b>".into(),
            fragment_size: 200,
            number_of_fragments: 5,
        };

        let rpc: RpcHighlightConfig = config.clone().into();
        let back: prism::backends::HighlightConfig = rpc.into();

        assert_eq!(back.fields, config.fields);
        assert_eq!(back.pre_tag, config.pre_tag);
        assert_eq!(back.post_tag, config.post_tag);
        assert_eq!(back.fragment_size, config.fragment_size);
        assert_eq!(back.number_of_fragments, config.number_of_fragments);
    }

    #[test]
    fn test_rpc_document_roundtrip() {
        let mut fields = HashMap::new();
        fields.insert("title".to_string(), Value::String("Hello".into()));
        fields.insert("count".to_string(), Value::Number(42.into()));
        fields.insert("active".to_string(), Value::Bool(true));

        let doc = prism::backends::Document {
            id: "doc-123".into(),
            fields: fields.clone(),
        };

        let rpc: RpcDocument = doc.into();
        let back: prism::backends::Document = rpc.into();

        assert_eq!(back.id, "doc-123");
        assert_eq!(back.fields, fields);
    }

    #[test]
    fn test_rpc_search_result_roundtrip() {
        let mut fields = HashMap::new();
        fields.insert("title".to_string(), Value::String("Test".into()));

        let mut highlight = HashMap::new();
        highlight.insert(
            "title".to_string(),
            vec!["<em>Test</em> doc".to_string()],
        );

        let result = prism::backends::SearchResult {
            id: "res-1".into(),
            score: 0.95,
            fields: fields.clone(),
            highlight: Some(highlight.clone()),
        };

        let rpc: RpcSearchResult = result.into();
        let back: prism::backends::SearchResult = rpc.into();

        assert_eq!(back.id, "res-1");
        assert!((back.score - 0.95).abs() < f32::EPSILON);
        assert_eq!(back.fields, fields);
        assert_eq!(back.highlight, Some(highlight));
    }

    #[test]
    fn test_rpc_search_result_no_highlight() {
        let result = prism::backends::SearchResult {
            id: "res-2".into(),
            score: 0.5,
            fields: HashMap::new(),
            highlight: None,
        };

        let rpc: RpcSearchResult = result.into();
        let back: prism::backends::SearchResult = rpc.into();

        assert!(back.highlight.is_none());
    }

    #[test]
    fn test_rpc_search_results_roundtrip() {
        let results = prism::backends::SearchResults {
            results: vec![
                prism::backends::SearchResult {
                    id: "a".into(),
                    score: 1.0,
                    fields: HashMap::new(),
                    highlight: None,
                },
                prism::backends::SearchResult {
                    id: "b".into(),
                    score: 0.8,
                    fields: HashMap::new(),
                    highlight: None,
                },
            ],
            total: 42,
            latency_ms: 15,
        };

        let rpc: RpcSearchResults = results.into();
        let back: prism::backends::SearchResults = rpc.into();

        assert_eq!(back.results.len(), 2);
        assert_eq!(back.total, 42);
        assert_eq!(back.latency_ms, 15);
        assert_eq!(back.results[0].id, "a");
        assert_eq!(back.results[1].id, "b");
    }

    #[test]
    fn test_rpc_backend_stats_roundtrip() {
        let stats = prism::backends::BackendStats {
            document_count: 1000,
            size_bytes: 1024 * 1024,
        };

        let rpc: RpcBackendStats = stats.into();
        let back: prism::backends::BackendStats = rpc.into();

        assert_eq!(back.document_count, 1000);
        assert_eq!(back.size_bytes, 1024 * 1024);
    }

    // --- Struct construction tests ---

    #[test]
    fn test_delete_by_query_request_serde() {
        let req = DeleteByQueryRequest {
            collection: "products".into(),
            query: RpcQuery {
                query_string: "obsolete:true".into(),
                fields: vec![],
                limit: 100,
                offset: 0,
                merge_strategy: None,
                text_weight: None,
                vector_weight: None,
                highlight: None,
                rrf_k: None,
                min_score: None,
                score_function: None,
                skip_ranking: false,
            },
            max_docs: 0,
            dry_run: true,
        };

        let json = serde_json::to_string(&req).unwrap();
        let back: DeleteByQueryRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.collection, "products");
        assert!(back.dry_run);
    }

    #[test]
    fn test_delete_by_query_response_serde() {
        let resp = DeleteByQueryResponse {
            deleted_count: 42,
            took_ms: 150,
            deleted_ids: vec!["a".into(), "b".into()],
        };

        let json = serde_json::to_string(&resp).unwrap();
        let back: DeleteByQueryResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.deleted_count, 42);
        assert_eq!(back.deleted_ids.len(), 2);
    }

    #[test]
    fn test_import_by_query_roundtrip() {
        let req = ImportByQueryRequest {
            source_collection: "old_index".into(),
            target_collection: "new_index".into(),
            query: RpcQuery {
                query_string: "*".into(),
                fields: vec![],
                limit: 1000,
                offset: 0,
                merge_strategy: None,
                text_weight: None,
                vector_weight: None,
                highlight: None,
                rrf_k: None,
                min_score: None,
                score_function: None,
                skip_ranking: false,
            },
            source_node: Some("node-1:9100".into()),
            batch_size: 500,
        };

        let json = serde_json::to_string(&req).unwrap();
        let back: ImportByQueryRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source_collection, "old_index");
        assert_eq!(back.source_node, Some("node-1:9100".into()));
    }

    #[test]
    fn test_node_info_full() {
        let info = NodeInfo {
            node_id: "node-1".into(),
            version: "0.7.0".into(),
            collections: vec!["a".into(), "b".into()],
            uptime_secs: 3600,
            healthy: true,
            protocol_version: 2,
            min_supported_version: 1,
            draining: false,
        };

        let json = serde_json::to_string(&info).unwrap();
        let back: NodeInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.collections.len(), 2);
        assert_eq!(back.protocol_version, 2);
    }

    #[test]
    fn test_shard_assignment_roundtrip() {
        let req = ShardAssignmentRequest {
            shard_id: "shard-0".into(),
            collection: "test".into(),
            primary_node: "node-1".into(),
            replica_nodes: vec!["node-2".into(), "node-3".into()],
            shard_number: 0,
        };

        let json = serde_json::to_string(&req).unwrap();
        let back: ShardAssignmentRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.replica_nodes.len(), 2);
    }

    #[test]
    fn test_cluster_health_serde() {
        let health = RpcClusterHealth {
            nodes: vec![RpcNodeHealth {
                node_id: "node-1".into(),
                state: "alive".into(),
                last_heartbeat: Some(1700000000),
                missed_heartbeats: 0,
                last_latency_ms: Some(5),
                draining: false,
            }],
            alive_count: 1,
            suspect_count: 0,
            dead_count: 0,
            total_count: 1,
            quorum_available: true,
        };

        let json = serde_json::to_string(&health).unwrap();
        let back: RpcClusterHealth = serde_json::from_str(&json).unwrap();
        assert!(back.quorum_available);
        assert_eq!(back.nodes.len(), 1);
    }

    #[test]
    fn test_schema_propagation_serde() {
        let req = RpcApplySchemaRequest {
            collection: "products".into(),
            version: 3,
            schema: serde_json::json!({"fields": {"title": "text"}}),
            created_at: 1700000000,
            created_by: "node-1".into(),
            changes: vec![RpcSchemaChange {
                change_type: "field_added".into(),
                path: "fields.title".into(),
                old_value: None,
                new_value: Some(Value::String("text".into())),
                description: "Added title field".into(),
            }],
            metadata: HashMap::new(),
        };

        let json = serde_json::to_string(&req).unwrap();
        let back: RpcApplySchemaRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, 3);
        assert_eq!(back.changes.len(), 1);
        assert_eq!(back.changes[0].change_type, "field_added");
    }
}
