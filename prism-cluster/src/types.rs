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

/// Information about a cluster node
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
}
