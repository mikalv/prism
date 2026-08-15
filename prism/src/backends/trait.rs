use crate::aggregations::AggregationResult;
use crate::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub fields: HashMap<String, Value>,
}

/// A single sort key. `field` may name a stored field or the special `_score`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortField {
    pub field: String,
    /// Ascending when true, descending when false.
    pub ascending: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub query_string: String,
    pub vector: Option<Vec<f32>>,
    pub fields: Vec<String>,
    pub limit: usize,
    pub offset: usize,
    // Optional runtime merge controls for hybrid searches
    pub merge_strategy: Option<String>, // "rrf" or "weighted"
    pub text_weight: Option<f32>,
    pub vector_weight: Option<f32>,
    /// Optional highlight configuration for search results
    pub highlight: Option<HighlightConfig>,
    /// Override RRF k parameter for this query
    pub rrf_k: Option<usize>,
    /// Minimum score threshold — results below this are filtered out
    pub min_score: Option<f32>,
    /// Ad-hoc score expression (e.g., "_score * 2")
    pub score_function: Option<String>,
    /// Skip ranking adjustments (used when hybrid coordinator calls text backend
    /// to avoid double-application of boosting)
    pub skip_ranking: bool,
    /// Sort keys applied in order. Empty means score-descending (default).
    pub sort: Vec<SortField>,
    /// Fields that must have a value (structured `exists` filter). Requires the
    /// field to be a fast field.
    pub exists_fields: Vec<String>,
    /// Fields that must NOT have a value (negated `exists`).
    pub not_exists_fields: Vec<String>,
    /// When true, populate `score_explanation` on each result with a
    /// per-stage breakdown of how its final score was computed. Off by
    /// default; the only runtime cost is a per-stage branch when enabled.
    pub explain: bool,
}

/// Configuration for search result highlighting
#[derive(Debug, Clone, Deserialize)]
pub struct HighlightConfig {
    /// Fields to generate highlights for
    pub fields: Vec<String>,
    /// Opening tag for highlighted terms (default: "<em>")
    #[serde(default = "default_pre_tag")]
    pub pre_tag: String,
    /// Closing tag for highlighted terms (default: "</em>")
    #[serde(default = "default_post_tag")]
    pub post_tag: String,
    /// Maximum number of characters per fragment (default: 150)
    #[serde(default = "default_fragment_size")]
    pub fragment_size: usize,
    /// Maximum number of fragments per field (default: 3)
    #[serde(default = "default_number_of_fragments")]
    pub number_of_fragments: usize,
}

fn default_pre_tag() -> String {
    "<em>".to_string()
}
fn default_post_tag() -> String {
    "</em>".to_string()
}
fn default_fragment_size() -> usize {
    150
}
fn default_number_of_fragments() -> usize {
    3
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, Value>,
    /// Highlighted snippets per field (only present when highlight is requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<HashMap<String, Vec<String>>>,
    /// Per-stage score breakdown (only present when `explain: true` was set
    /// on the request).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_explanation: Option<crate::ranking::ScoreExplanation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResultsWithAggs {
    pub results: Vec<SearchResult>,
    pub total: u64,
    pub aggregations: HashMap<String, AggregationResult>,
}

#[async_trait]
pub trait SearchBackend: Send + Sync {
    /// Index documents
    async fn index(&self, collection: &str, docs: Vec<Document>) -> Result<()>;

    /// Search documents
    async fn search(&self, collection: &str, query: Query) -> Result<SearchResults>;

    /// Get document by ID
    async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>>;

    /// Delete documents by IDs
    async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<()>;

    /// Get backend statistics
    async fn stats(&self, collection: &str) -> Result<BackendStats>;

    /// Search documents with aggregations
    async fn search_with_aggs(
        &self,
        collection: &str,
        query: &Query,
        aggregations: Vec<crate::aggregations::AggregationRequest>,
    ) -> Result<SearchResultsWithAggs>;

    /// Fetch stored embedding vectors for the given document ids, when the
    /// backend has them. Missing ids are simply absent from the map. The default
    /// returns an empty map (text-only backends store no vectors). Used by
    /// semantic near-duplicate collapse.
    async fn get_vectors(
        &self,
        _collection: &str,
        _ids: &[String],
    ) -> Result<HashMap<String, Vec<f32>>> {
        Ok(HashMap::new())
    }
}

#[derive(Debug, Clone)]
pub struct BackendStats {
    pub document_count: usize,
    pub size_bytes: usize,
}
