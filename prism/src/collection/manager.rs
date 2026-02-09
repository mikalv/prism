use crate::backends::{
    BackendStats, Document, HybridSearchCoordinator, Query, SearchBackend, SearchResults,
    SearchResultsWithAggs, TextBackend, VectorBackend,
};
use crate::schema::{CollectionSchema, SchemaLoader};
use crate::{Error, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct CollectionManager {
    schemas: RwLock<HashMap<String, CollectionSchema>>,
    per_collection_backends: RwLock<HashMap<String, Arc<dyn SearchBackend>>>,
    text_backend: Arc<TextBackend>,
    vector_backend: Arc<VectorBackend>,
}

impl CollectionManager {
    pub fn new(
        schemas_dir: impl AsRef<Path>,
        text_backend: Arc<TextBackend>,
        vector_backend: Arc<VectorBackend>,
    ) -> Result<Self> {
        let loader = SchemaLoader::new(schemas_dir);
        let schemas = loader.load_all()?;

        // Lint schemas at runtime and fail fast if critical issues found
        let lint_issues = SchemaLoader::lint_all(&schemas);
        if !lint_issues.is_empty() {
            // Aggregate messages
            let mut msgs = Vec::new();
            for (col, issues) in &lint_issues {
                for issue in issues {
                    msgs.push(format!("{}: {}", col, issue));
                }
            }
            return Err(Error::Schema(format!(
                "Schema lint errors:\n{}",
                msgs.join("\n")
            )));
        }

        let mut per_collection_backends = HashMap::new();
        for (name, schema) in &schemas {
            let backend =
                Self::build_backend_for_schema(schema, &text_backend, &vector_backend)?;
            if let Some(b) = backend {
                per_collection_backends.insert(name.clone(), b);
            }
        }

        Ok(Self {
            schemas: RwLock::new(schemas),
            per_collection_backends: RwLock::new(per_collection_backends),
            text_backend: text_backend.clone(),
            vector_backend: vector_backend.clone(),
        })
    }

    /// Build the appropriate SearchBackend for a given schema.
    fn build_backend_for_schema(
        schema: &CollectionSchema,
        text_backend: &Arc<TextBackend>,
        vector_backend: &Arc<VectorBackend>,
    ) -> Result<Option<Arc<dyn SearchBackend>>> {
        let use_text = schema.backends.text.is_some();
        let use_vector = schema.backends.vector.is_some();

        if use_text && use_vector {
            let vw = schema
                .backends
                .vector
                .as_ref()
                .map(|v| v.vector_weight)
                .unwrap_or(0.5);
            if !(0.0..=1.0).contains(&vw) {
                return Err(Error::Schema(format!(
                    "vector_weight must be between 0.0 and 1.0 for collection {}",
                    schema.collection
                )));
            }
            let hybrid =
                HybridSearchCoordinator::new(text_backend.clone(), vector_backend.clone(), vw);
            Ok(Some(Arc::new(hybrid) as Arc<dyn SearchBackend>))
        } else if use_text {
            Ok(Some(text_backend.clone() as Arc<dyn SearchBackend>))
        } else if use_vector {
            Ok(Some(vector_backend.clone() as Arc<dyn SearchBackend>))
        } else {
            Ok(None)
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        let schemas = self.schemas.read().clone();
        for (name, schema) in &schemas {
            // Storage is configured at backend construction time via SegmentStorage.
            // Initialize just sets up the collection schema/indexes.
            if schema.backends.text.is_some() {
                self.text_backend.initialize(name, schema).await?;
            }
            if schema.backends.vector.is_some() {
                self.vector_backend.initialize(name, schema).await?;
            }
        }

        // Update collections count gauge
        metrics::gauge!("prism_collections_count").set(schemas.len() as f64);

        Ok(())
    }

    pub async fn index(&self, collection: &str, docs: Vec<Document>) -> Result<()> {
        let (has_backend, has_text) = {
            let schemas = self.schemas.read();
            let schema = schemas
                .get(collection)
                .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;
            let has_text = schema.backends.text.is_some();
            let backends = self.per_collection_backends.read();
            let has_backend = backends.contains_key(collection);
            (has_backend, has_text)
        };

        if has_backend {
            let backend = self.per_collection_backends.read().get(collection).cloned();
            if let Some(backend) = backend {
                backend.index(collection, docs).await?;
                return Ok(());
            }
        }

        // Fallback: try text backend
        if has_text {
            self.text_backend.index(collection, docs).await?;
        }

        Ok(())
    }

    pub async fn search(&self, collection: &str, query: Query) -> Result<SearchResults> {
        let (backend, has_text) = {
            let schemas = self.schemas.read();
            let schema = schemas
                .get(collection)
                .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;
            let has_text = schema.backends.text.is_some();
            let backends = self.per_collection_backends.read();
            let backend = backends.get(collection).cloned();
            (backend, has_text)
        };

        if let Some(backend) = backend {
            return backend.search(collection, query).await;
        }

        if has_text {
            return self.text_backend.search(collection, query).await;
        }

        Err(Error::Backend(
            "No backend available for collection".to_string(),
        ))
    }

    pub async fn search_with_aggs(
        &self,
        collection: &str,
        query: &Query,
        aggregations: Vec<crate::aggregations::AggregationRequest>,
    ) -> Result<SearchResultsWithAggs> {
        let (backend, has_text) = {
            let schemas = self.schemas.read();
            let schema = schemas
                .get(collection)
                .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;
            let has_text = schema.backends.text.is_some();
            let backends = self.per_collection_backends.read();
            let backend = backends.get(collection).cloned();
            (backend, has_text)
        };

        if let Some(backend) = backend {
            return backend
                .search_with_aggs(collection, query, aggregations)
                .await;
        }

        if has_text {
            return self
                .text_backend
                .search_with_aggs(collection, query, aggregations)
                .await;
        }

        Err(Error::Backend(
            "No backend available for collection".to_string(),
        ))
    }

    pub async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>> {
        let (backend, has_text) = {
            let schemas = self.schemas.read();
            let schema = schemas
                .get(collection)
                .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;
            let has_text = schema.backends.text.is_some();
            let backends = self.per_collection_backends.read();
            let backend = backends.get(collection).cloned();
            (backend, has_text)
        };

        if let Some(backend) = backend {
            return backend.get(collection, id).await;
        }

        if has_text {
            return self.text_backend.get(collection, id).await;
        }

        Ok(None)
    }

    pub async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<()> {
        let (backend, has_text) = {
            let schemas = self.schemas.read();
            let schema = schemas
                .get(collection)
                .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;
            let has_text = schema.backends.text.is_some();
            let backends = self.per_collection_backends.read();
            let backend = backends.get(collection).cloned();
            (backend, has_text)
        };

        if let Some(backend) = backend {
            backend.delete(collection, ids).await?;
            return Ok(());
        }

        if has_text {
            self.text_backend.delete(collection, ids).await?;
        }

        Ok(())
    }

    pub async fn stats(&self, collection: &str) -> Result<BackendStats> {
        let (backend, has_text) = {
            let schemas = self.schemas.read();
            let schema = schemas
                .get(collection)
                .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;
            let has_text = schema.backends.text.is_some();
            let backends = self.per_collection_backends.read();
            let backend = backends.get(collection).cloned();
            (backend, has_text)
        };

        if let Some(backend) = backend {
            return backend.stats(collection).await;
        }

        if has_text {
            return self.text_backend.stats(collection).await;
        }

        Err(Error::Backend(
            "No backend available for collection".to_string(),
        ))
    }

    pub fn list_collections(&self) -> Vec<String> {
        self.schemas.read().keys().cloned().collect()
    }

    pub fn get_schema(&self, collection: &str) -> Option<CollectionSchema> {
        self.schemas.read().get(collection).cloned()
    }

    /// Run schema linting at runtime and return map collection -> issues
    pub fn lint_schemas(&self) -> std::collections::HashMap<String, Vec<String>> {
        let schemas = self.schemas.read();
        let mut map = std::collections::HashMap::new();
        for (name, schema) in schemas.iter() {
            let issues = crate::schema::loader::SchemaLoader::lint_schema(schema);
            if !issues.is_empty() {
                map.insert(name.clone(), issues);
            }
        }
        map
    }

    /// Generate an embedding for query text using the collection's configured embedder.
    /// Requires embedding provider to be configured on the vector backend.
    pub async fn embed_query(&self, _collection: &str, text: &str) -> Result<Vec<f32>> {
        self.vector_backend.embed_text(text).await
    }

    /// Get embedding cache statistics (if cache is enabled)
    pub async fn cache_stats(&self) -> Option<crate::cache::EmbeddingCacheStats> {
        self.vector_backend.embedding_cache_stats().await
    }

    // ========================================================================
    // Runtime collection management (Issue #57)
    // ========================================================================

    /// Remove a collection from the running server.
    ///
    /// Unloads the collection from all in-memory structures (schemas, backends,
    /// text index, vector index). Does NOT delete data from disk.
    pub async fn remove_collection(&self, name: &str) -> Result<()> {
        // Verify collection exists
        {
            let schemas = self.schemas.read();
            if !schemas.contains_key(name) {
                return Err(Error::CollectionNotFound(name.to_string()));
            }
        }

        // Remove from backends first (may need async for vector persist)
        self.text_backend.remove_collection(name);
        self.vector_backend.remove_collection(name).await?;

        // Remove from manager maps
        self.per_collection_backends.write().remove(name);
        self.schemas.write().remove(name);

        // Update gauge
        metrics::gauge!("prism_collections_count").set(self.schemas.read().len() as f64);

        tracing::info!("Collection '{}' removed from running server", name);
        Ok(())
    }

    /// Add a collection to the running server from a schema.
    ///
    /// Lints the schema, creates backend routing, and initializes indexes.
    pub async fn add_collection(&self, schema: CollectionSchema) -> Result<()> {
        let name = schema.collection.clone();

        // Lint the schema
        let issues = SchemaLoader::lint_schema(&schema);
        if !issues.is_empty() {
            return Err(Error::Schema(format!(
                "Schema lint errors for '{}': {}",
                name,
                issues.join("; ")
            )));
        }

        // Build backend
        let backend =
            Self::build_backend_for_schema(&schema, &self.text_backend, &self.vector_backend)?;

        // Initialize backend indexes
        if schema.backends.text.is_some() {
            self.text_backend.initialize(&name, &schema).await?;
        }
        if schema.backends.vector.is_some() {
            self.vector_backend.initialize(&name, &schema).await?;
        }

        // Insert into manager maps
        if let Some(b) = backend {
            self.per_collection_backends.write().insert(name.clone(), b);
        }
        self.schemas.write().insert(name.clone(), schema);

        // Update gauge
        metrics::gauge!("prism_collections_count").set(self.schemas.read().len() as f64);

        tracing::info!("Collection '{}' added to running server", name);
        Ok(())
    }

    /// Get a reference to the text backend (for use by detach/attach operations).
    pub fn text_backend(&self) -> &Arc<TextBackend> {
        &self.text_backend
    }

    /// Get a reference to the vector backend (for use by detach/attach operations).
    pub fn vector_backend(&self) -> &Arc<VectorBackend> {
        &self.vector_backend
    }

    /// Perform hybrid search combining text and vector search results.
    ///
    /// # Arguments
    /// * `collection` - Collection name
    /// * `text_query` - Text query string for full-text search
    /// * `vector` - Optional vector for semantic search
    /// * `limit` - Maximum number of results
    /// * `merge_strategy` - "rrf" (default) or "weighted"
    /// * `text_weight` - Weight for text results in weighted merge (default 0.5)
    /// * `vector_weight` - Weight for vector results in weighted merge (default 0.5)
    pub async fn hybrid_search(
        &self,
        collection: &str,
        text_query: &str,
        vector: Option<Vec<f32>>,
        limit: usize,
        merge_strategy: Option<&str>,
        text_weight: Option<f32>,
        vector_weight: Option<f32>,
    ) -> Result<SearchResults> {
        let (has_text, has_vector) = {
            let schemas = self.schemas.read();
            let schema = schemas
                .get(collection)
                .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;
            (
                schema.backends.text.is_some(),
                schema.backends.vector.is_some(),
            )
        };

        // Text-only search
        if !has_vector || vector.is_none() {
            let query = Query {
                query_string: text_query.to_string(),
                fields: vec![],
                limit,
                offset: 0,
                merge_strategy: None,
                text_weight: None,
                vector_weight: None,
                highlight: None,
            };
            return self.text_backend.search(collection, query).await;
        }

        // Vector-only search (no text backend configured)
        if !has_text {
            let vec = vector.unwrap();
            let query = Query {
                query_string: serde_json::to_string(&vec).unwrap_or_default(),
                fields: vec![],
                limit,
                offset: 0,
                merge_strategy: None,
                text_weight: None,
                vector_weight: None,
                highlight: None,
            };
            return self.vector_backend.search(collection, query).await;
        }

        // Hybrid search: run both and merge
        let vec = vector.unwrap();

        let text_query_obj = Query {
            query_string: text_query.to_string(),
            fields: vec![],
            limit,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
        };

        let vec_query_obj = Query {
            query_string: serde_json::to_string(&vec).unwrap_or_default(),
            fields: vec![],
            limit,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
        };

        // Run searches in parallel
        let (text_results, vec_results) = tokio::join!(
            self.text_backend.search(collection, text_query_obj),
            self.vector_backend.search(collection, vec_query_obj)
        );

        let text_results = text_results?;
        let vec_results = vec_results?;

        // Merge results
        let merged = match merge_strategy {
            Some("weighted") => {
                let tw = text_weight.unwrap_or(0.5);
                let vw = vector_weight.unwrap_or(0.5);
                HybridSearchCoordinator::merge_weighted_public(
                    text_results,
                    vec_results,
                    tw,
                    vw,
                    limit,
                )
            }
            _ => {
                // Default to RRF
                HybridSearchCoordinator::merge_rrf_public(text_results, vec_results, 60, limit)
            }
        };

        Ok(merged)
    }

    // ========================================================================
    // Index Inspection API (Issue #24)
    // ========================================================================

    /// Get top-k most frequent terms for a field.
    pub fn get_top_terms(
        &self,
        collection: &str,
        field: &str,
        limit: usize,
    ) -> Result<Vec<crate::backends::text::TermInfo>> {
        self.text_backend.get_top_terms(collection, field, limit)
    }

    /// Find documents similar to a given document or text.
    pub fn more_like_this(
        &self,
        collection: &str,
        doc_id: Option<&str>,
        like_text: Option<&str>,
        fields: &[String],
        min_term_freq: usize,
        min_doc_freq: u64,
        max_query_terms: usize,
        size: usize,
    ) -> Result<SearchResults> {
        self.text_backend.more_like_this(
            collection,
            doc_id,
            like_text,
            fields,
            min_term_freq,
            min_doc_freq,
            max_query_terms,
            size,
        )
    }

    /// Suggest terms from the index using prefix matching and optional fuzzy correction.
    pub fn suggest(
        &self,
        collection: &str,
        field: &str,
        prefix: &str,
        size: usize,
        fuzzy: bool,
        max_distance: usize,
    ) -> Result<Vec<crate::backends::text::SuggestEntry>> {
        self.text_backend
            .suggest_terms(collection, field, prefix, size, fuzzy, max_distance)
    }

    /// Get segment information for a collection.
    pub fn get_segments(&self, collection: &str) -> Result<crate::backends::text::SegmentsInfo> {
        self.text_backend.get_segments(collection)
    }

    /// Reconstruct a document showing stored fields and indexed terms.
    pub fn reconstruct_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<crate::backends::text::ReconstructedDocument>> {
        self.text_backend.reconstruct_document(collection, id)
    }

    // ========================================================================
    // Multi-Collection Search (Issue #74)
    // ========================================================================

    /// Expand collection patterns to matching collection names.
    /// Supports wildcards like "logs-2026-*" or "products-*".
    pub fn expand_collection_patterns(&self, patterns: &[String]) -> Vec<String> {
        let schemas = self.schemas.read();
        let mut result = Vec::new();
        let all_collections: Vec<&String> = schemas.keys().collect();

        for pattern in patterns {
            if pattern.contains('*') {
                // Simple glob matching: * matches any sequence of characters
                for collection in &all_collections {
                    if Self::glob_match(pattern, collection) && !result.contains(*collection) {
                        result.push((*collection).clone());
                    }
                }
            } else if schemas.contains_key(pattern) && !result.contains(pattern) {
                result.push(pattern.clone());
            }
        }

        result
    }

    /// Simple glob pattern matching supporting only '*' wildcard.
    fn glob_match(pattern: &str, text: &str) -> bool {
        let pattern_chars: Vec<char> = pattern.chars().collect();
        let text_chars: Vec<char> = text.chars().collect();

        let mut dp = vec![vec![false; text_chars.len() + 1]; pattern_chars.len() + 1];
        dp[0][0] = true;

        // Handle leading wildcards
        for (i, &p) in pattern_chars.iter().enumerate() {
            if p == '*' {
                dp[i + 1][0] = dp[i][0];
            }
        }

        for (i, &p) in pattern_chars.iter().enumerate() {
            for (j, &t) in text_chars.iter().enumerate() {
                if p == '*' {
                    // * can match zero or more characters
                    dp[i + 1][j + 1] = dp[i][j + 1] || dp[i + 1][j];
                } else if p == '?' || p == t {
                    dp[i + 1][j + 1] = dp[i][j];
                }
            }
        }

        dp[pattern_chars.len()][text_chars.len()]
    }

    /// Search across multiple collections and merge results using RRF.
    ///
    /// # Arguments
    /// * `collections` - List of collection names or patterns (supports wildcards like "logs-*")
    /// * `query` - Search query
    /// * `rrf_k` - RRF constant (default 60, higher reduces rank influence)
    ///
    /// Returns merged search results with `_collection` field added to each result.
    pub async fn multi_search(
        &self,
        collections: &[String],
        query: Query,
        rrf_k: Option<usize>,
    ) -> Result<MultiSearchResults> {
        let start = std::time::Instant::now();
        let expanded = self.expand_collection_patterns(collections);

        if expanded.is_empty() {
            return Ok(MultiSearchResults {
                results: vec![],
                total: 0,
                collections_searched: vec![],
                latency_ms: 0,
            });
        }

        // Run searches in parallel across all collections
        let search_futures: Vec<_> = expanded
            .iter()
            .map(|collection| {
                let q = query.clone();
                let col = collection.clone();
                async move {
                    let result = self.search(&col, q).await;
                    (col, result)
                }
            })
            .collect();

        let results = futures::future::join_all(search_futures).await;

        // Collect successful results and add collection info
        let mut all_results: Vec<(String, SearchResults)> = Vec::new();
        let mut collections_searched = Vec::new();
        let mut errors = Vec::new();

        for (collection, result) in results {
            match result {
                Ok(search_results) => {
                    collections_searched.push(collection.clone());
                    all_results.push((collection, search_results));
                }
                Err(e) => {
                    errors.push((collection, e));
                }
            }
        }

        // Log any errors (but don't fail the entire request)
        for (collection, error) in errors {
            tracing::warn!(
                "Multi-search: error in collection '{}': {:?}",
                collection,
                error
            );
        }

        // Merge results using RRF
        let k = rrf_k.unwrap_or(60);
        let merged = Self::merge_multi_collection_rrf(all_results, k, query.limit);

        let latency_ms = start.elapsed().as_millis() as u64;

        Ok(MultiSearchResults {
            results: merged.results,
            total: merged.total,
            collections_searched,
            latency_ms,
        })
    }

    /// Merge results from multiple collections using Reciprocal Rank Fusion.
    /// Each result gets a `_collection` field indicating its source.
    fn merge_multi_collection_rrf(
        collection_results: Vec<(String, SearchResults)>,
        k: usize,
        limit: usize,
    ) -> MultiSearchMergedResults {
        use std::collections::HashMap;

        let mut scores: HashMap<String, f32> = HashMap::new();
        let mut result_data: HashMap<String, MultiSearchResult> = HashMap::new();

        for (collection, search_results) in collection_results {
            for (rank, result) in search_results.results.into_iter().enumerate() {
                let rank_score = 1.0_f32 / ((k as f32) + ((rank + 1) as f32));

                // Create unique key combining collection and doc ID
                let unique_key = format!("{}:{}", collection, result.id);

                *scores.entry(unique_key.clone()).or_insert(0.0) += rank_score;

                result_data
                    .entry(unique_key)
                    .or_insert_with(|| MultiSearchResult {
                        id: result.id,
                        collection: collection.clone(),
                        score: 0.0,
                        fields: result.fields,
                        highlight: result.highlight,
                    });
            }
        }

        // Apply final scores
        let mut merged: Vec<MultiSearchResult> = result_data
            .into_iter()
            .map(|(key, mut result)| {
                result.score = scores.get(&key).copied().unwrap_or(0.0);
                result
            })
            .collect();

        // Sort by score descending
        merged.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total = merged.len();
        merged.truncate(limit);

        MultiSearchMergedResults {
            results: merged,
            total,
        }
    }
}

/// Result from multi-collection search with collection source info
#[derive(Debug, Clone, serde::Serialize)]
pub struct MultiSearchResult {
    pub id: String,
    #[serde(rename = "_collection")]
    pub collection: String,
    pub score: f32,
    pub fields: std::collections::HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<std::collections::HashMap<String, Vec<String>>>,
}

/// Response for multi-collection search
#[derive(Debug, Clone, serde::Serialize)]
pub struct MultiSearchResults {
    pub results: Vec<MultiSearchResult>,
    pub total: usize,
    pub collections_searched: Vec<String>,
    pub latency_ms: u64,
}

/// Internal struct for RRF merging
struct MultiSearchMergedResults {
    results: Vec<MultiSearchResult>,
    total: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::TextBackend;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_collection_manager_search() -> Result<()> {
        let temp = TempDir::new()?;
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");

        std::fs::create_dir_all(&schemas_dir)?;

        // Write schema
        fs::write(
            schemas_dir.join("articles.yaml"),
            r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
      - name: content
        type: text
        indexed: true
"#,
        )?;

        let text_backend = Arc::new(TextBackend::new(&data_dir)?);
        let vector_backend = Arc::new(VectorBackend::new(&data_dir)?);
        let manager = CollectionManager::new(&schemas_dir, text_backend, vector_backend)?;
        manager.initialize().await?;

        // Index document
        let doc = Document {
            id: "article1".to_string(),
            fields: HashMap::from([
                ("title".to_string(), json!("Rust Programming")),
                ("content".to_string(), json!("Learn Rust today")),
            ]),
        };

        manager.index("articles", vec![doc]).await?;

        // Search
        let query = Query {
            query_string: "rust".to_string(),
            fields: vec!["title".to_string(), "content".to_string()],
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
        };

        let results = manager.search("articles", query).await?;
        assert!(results.total > 0);

        Ok(())
    }

    #[test]
    fn test_glob_match() {
        // Test exact match
        assert!(CollectionManager::glob_match("articles", "articles"));
        assert!(!CollectionManager::glob_match("articles", "products"));

        // Test wildcard at end
        assert!(CollectionManager::glob_match("logs-*", "logs-2026"));
        assert!(CollectionManager::glob_match("logs-*", "logs-2026-01"));
        assert!(!CollectionManager::glob_match("logs-*", "articles"));

        // Test wildcard at beginning
        assert!(CollectionManager::glob_match("*-products", "us-products"));
        assert!(CollectionManager::glob_match("*-products", "eu-products"));
        assert!(!CollectionManager::glob_match("*-products", "products-us"));

        // Test wildcard in middle
        assert!(CollectionManager::glob_match(
            "logs-*-backup",
            "logs-2026-backup"
        ));
        assert!(!CollectionManager::glob_match("logs-*-backup", "logs-2026"));

        // Test multiple wildcards
        assert!(CollectionManager::glob_match("*-logs-*", "us-logs-2026"));
        assert!(CollectionManager::glob_match("*-*", "a-b"));

        // Test empty patterns
        assert!(CollectionManager::glob_match("*", "anything"));
        assert!(CollectionManager::glob_match("*", ""));
    }

    #[tokio::test]
    async fn test_multi_search() -> Result<()> {
        let temp = TempDir::new()?;
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");

        std::fs::create_dir_all(&schemas_dir)?;

        // Create two collections
        fs::write(
            schemas_dir.join("products.yaml"),
            r#"
collection: products
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
"#,
        )?;

        fs::write(
            schemas_dir.join("articles.yaml"),
            r#"
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
"#,
        )?;

        let text_backend = Arc::new(TextBackend::new(&data_dir)?);
        let vector_backend = Arc::new(VectorBackend::new(&data_dir)?);
        let manager = CollectionManager::new(&schemas_dir, text_backend, vector_backend)?;
        manager.initialize().await?;

        // Index documents in both collections
        manager
            .index(
                "products",
                vec![Document {
                    id: "p1".to_string(),
                    fields: HashMap::from([("title".to_string(), json!("Rust Book"))]),
                }],
            )
            .await?;

        manager
            .index(
                "articles",
                vec![Document {
                    id: "a1".to_string(),
                    fields: HashMap::from([("title".to_string(), json!("Learning Rust"))]),
                }],
            )
            .await?;

        // Multi-search across both collections
        let query = Query {
            query_string: "rust".to_string(),
            fields: vec!["title".to_string()],
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
        };

        let results = manager
            .multi_search(
                &["products".to_string(), "articles".to_string()],
                query,
                None,
            )
            .await?;

        assert_eq!(results.total, 2);
        assert_eq!(results.collections_searched.len(), 2);

        // Verify results include collection info
        let collections: Vec<&String> = results.results.iter().map(|r| &r.collection).collect();
        assert!(collections.contains(&&"products".to_string()));
        assert!(collections.contains(&&"articles".to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_search_with_wildcard() -> Result<()> {
        let temp = TempDir::new()?;
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");

        std::fs::create_dir_all(&schemas_dir)?;

        // Create collections with pattern naming
        for month in ["01", "02", "03"] {
            fs::write(
                schemas_dir.join(format!("logs-2026-{}.yaml", month)),
                format!(
                    r#"
collection: logs-2026-{}
backends:
  text:
    fields:
      - name: message
        type: text
        indexed: true
        stored: true
"#,
                    month
                ),
            )?;
        }

        let text_backend = Arc::new(TextBackend::new(&data_dir)?);
        let vector_backend = Arc::new(VectorBackend::new(&data_dir)?);
        let manager = CollectionManager::new(&schemas_dir, text_backend, vector_backend)?;
        manager.initialize().await?;

        // Test pattern expansion
        let expanded = manager.expand_collection_patterns(&["logs-2026-*".to_string()]);
        assert_eq!(expanded.len(), 3);

        // Test non-matching pattern
        let empty = manager.expand_collection_patterns(&["nonexistent-*".to_string()]);
        assert!(empty.is_empty());

        Ok(())
    }
}
