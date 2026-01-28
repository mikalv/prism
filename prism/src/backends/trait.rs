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

#[derive(Debug, Clone)]
pub struct Query {
    pub query_string: String,
    pub fields: Vec<String>,
    pub limit: usize,
    pub offset: usize,
    // Optional runtime merge controls for hybrid searches
    pub merge_strategy: Option<String>, // "rrf" or "weighted"
    pub text_weight: Option<f32>,
    pub vector_weight: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, Value>,
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
}

#[derive(Debug, Clone)]
pub struct BackendStats {
    pub document_count: usize,
    pub size_bytes: usize,
}
