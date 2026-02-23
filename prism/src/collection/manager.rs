use crate::backends::{
    BackendStats, Document, HybridSearchCoordinator, Query, SearchBackend, SearchResults,
    SearchResultsWithAggs, ShardedGraphBackend, TextBackend, VectorBackend,
};
use crate::ranking::reranker::{RerankOptions, Reranker};
use crate::schema::{CollectionSchema, SchemaLoader};
use crate::{Error, Result};
use parking_lot::RwLock;
use prism_storage::SegmentStorage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct CollectionManager {
    schemas: RwLock<HashMap<String, CollectionSchema>>,
    per_collection_backends: RwLock<HashMap<String, Arc<dyn SearchBackend>>>,
    per_collection_rerankers: RwLock<HashMap<String, Arc<dyn Reranker>>>,
    per_collection_graphs: RwLock<HashMap<String, Arc<ShardedGraphBackend>>>,
    text_backend: Arc<TextBackend>,
    vector_backend: Arc<VectorBackend>,
    graph_storage: Option<Arc<dyn SegmentStorage>>,
    schemas_dir: PathBuf,
}

impl CollectionManager {
    pub fn new(
        schemas_dir: impl AsRef<Path>,
        text_backend: Arc<TextBackend>,
        vector_backend: Arc<VectorBackend>,
        graph_storage: Option<Arc<dyn SegmentStorage>>,
    ) -> Result<Self> {
        let schemas_dir_path = schemas_dir.as_ref().to_path_buf();
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
        let mut per_collection_rerankers = HashMap::new();
        let mut per_collection_graphs = HashMap::new();
        for (name, schema) in &schemas {
            let backend = Self::build_backend_for_schema(schema, &text_backend, &vector_backend)?;
            if let Some(b) = backend {
                per_collection_backends.insert(name.clone(), b);
            }
            if let Some(reranker) = Self::build_reranker_for_schema(schema) {
                per_collection_rerankers.insert(name.clone(), reranker);
            }
            if let Some(ref graph_config) = schema.backends.graph {
                let graph = ShardedGraphBackend::new(name, graph_config, graph_storage.clone());
                per_collection_graphs.insert(name.clone(), Arc::new(graph));
            }
        }

        Ok(Self {
            schemas: RwLock::new(schemas),
            per_collection_backends: RwLock::new(per_collection_backends),
            per_collection_rerankers: RwLock::new(per_collection_rerankers),
            per_collection_graphs: RwLock::new(per_collection_graphs),
            text_backend: text_backend.clone(),
            vector_backend: vector_backend.clone(),
            graph_storage,
            schemas_dir: schemas_dir_path,
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
            let distance_metric = schema.backends.vector.as_ref().map(|v| v.distance.clone());
            let hybrid = if let Some(config) = &schema.hybrid {
                HybridSearchCoordinator::with_config(
                    text_backend.clone(),
                    vector_backend.clone(),
                    vw,
                    config,
                    distance_metric,
                )
            } else {
                HybridSearchCoordinator::new(text_backend.clone(), vector_backend.clone(), vw)
            };
            Ok(Some(Arc::new(hybrid) as Arc<dyn SearchBackend>))
        } else if use_text {
            Ok(Some(text_backend.clone() as Arc<dyn SearchBackend>))
        } else if use_vector {
            Ok(Some(vector_backend.clone() as Arc<dyn SearchBackend>))
        } else {
            Ok(None)
        }
    }

    /// Build a reranker from schema configuration. Returns None if no reranking is configured.
    fn build_reranker_for_schema(schema: &CollectionSchema) -> Option<Arc<dyn Reranker>> {
        let config = schema.reranking.as_ref()?;

        match config.reranker_type {
            crate::schema::RerankerType::ScoreFunction => {
                let expr = config.score_function.as_deref().unwrap_or("_score");
                match crate::ranking::ScoreFunctionReranker::new(expr) {
                    Ok(r) => {
                        tracing::info!(
                            "Built ScoreFunctionReranker for '{}': {}",
                            schema.collection,
                            expr
                        );
                        Some(Arc::new(r))
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to build ScoreFunctionReranker for '{}': {}",
                            schema.collection,
                            e
                        );
                        None
                    }
                }
            }
            crate::schema::RerankerType::CrossEncoder => {
                // CrossEncoder requires async construction (model download).
                // Return None here; the reranker will be built in initialize_rerankers()
                // or add_collection().
                tracing::info!(
                    "CrossEncoder reranker for '{}' deferred to async initialization",
                    schema.collection,
                );
                None
            }
        }
    }

    /// Initialize cross-encoder rerankers asynchronously (called from initialize())
    async fn initialize_rerankers(&self) {
        let schemas = self.schemas.read().clone();
        for (name, schema) in &schemas {
            if let Some(config) = &schema.reranking {
                if config.reranker_type == crate::schema::RerankerType::CrossEncoder {
                    let ce_config = config.cross_encoder.as_ref();
                    let model_id = ce_config
                        .map(|c| c.model_id.as_str())
                        .unwrap_or("cross-encoder/ms-marco-MiniLM-L-6-v2");
                    let max_length = ce_config.map(|c| c.max_length).unwrap_or(512);

                    match crate::ranking::CrossEncoderReranker::new(model_id, max_length).await {
                        Ok(reranker) => {
                            tracing::info!(
                                "Initialized CrossEncoder reranker for '{}' (model: {})",
                                name,
                                model_id
                            );
                            self.per_collection_rerankers
                                .write()
                                .insert(name.clone(), Arc::new(reranker));
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to initialize CrossEncoder reranker for '{}': {}. \
                                 Searches will proceed without reranking.",
                                name,
                                e
                            );
                        }
                    }
                }
            }
        }
    }

    /// Resolve reranking configuration by merging schema defaults with per-request override.
    fn resolve_rerank_config(
        schema: &CollectionSchema,
        override_opts: Option<&RerankOptions>,
    ) -> Option<ResolvedRerankConfig> {
        // If there's an explicit override that disables reranking, return None
        if let Some(opts) = override_opts {
            if !opts.enabled {
                return None;
            }
        }

        // Check schema config
        let schema_config = schema.reranking.as_ref();

        // If no schema config and no override enabling it, no reranking
        if schema_config.is_none() && override_opts.is_none() {
            return None;
        }

        // If override exists with enabled=true but no schema config, use override defaults
        let candidates = override_opts
            .map(|o| o.candidates)
            .or_else(|| schema_config.map(|c| c.candidates))
            .unwrap_or(100);

        let text_fields = override_opts
            .and_then(|o| {
                if o.text_fields.is_empty() {
                    None
                } else {
                    Some(o.text_fields.clone())
                }
            })
            .or_else(|| {
                schema_config.and_then(|c| {
                    if c.text_fields.is_empty() {
                        None
                    } else {
                        Some(c.text_fields.clone())
                    }
                })
            })
            .unwrap_or_default();

        Some(ResolvedRerankConfig {
            candidates,
            text_fields,
        })
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

        // Initialize graph backends
        let graph_entries: Vec<_> = {
            let graphs = self.per_collection_graphs.read();
            graphs.iter().map(|(n, g)| (n.clone(), g.clone())).collect()
        };
        for (name, graph) in &graph_entries {
            graph.initialize().await?;
            tracing::info!(
                "Initialized graph backend for '{}' ({} shards)",
                name,
                graph.num_shards()
            );
        }

        // Initialize async rerankers (cross-encoders)
        self.initialize_rerankers().await;

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

    pub async fn search(
        &self,
        collection: &str,
        query: Query,
        rerank_override: Option<&RerankOptions>,
    ) -> Result<SearchResults> {
        let (backend, has_text, schema) = {
            let schemas = self.schemas.read();
            let schema = schemas
                .get(collection)
                .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?
                .clone();
            let has_text = schema.backends.text.is_some();
            let backends = self.per_collection_backends.read();
            let backend = backends.get(collection).cloned();
            (backend, has_text, schema)
        };

        // Resolve reranking config
        let rerank_config = Self::resolve_rerank_config(&schema, rerank_override);
        let reranker = if rerank_config.is_some() {
            self.per_collection_rerankers
                .read()
                .get(collection)
                .cloned()
        } else {
            None
        };

        // Phase 1: Retrieve candidates
        let original_limit = query.limit;
        let query_string_for_rerank = query.query_string.clone();
        let mut phase1_query = query;
        if let (Some(ref config), Some(_)) = (&rerank_config, &reranker) {
            // Expand limit to retrieve more candidates for reranking
            phase1_query.limit = config.candidates.max(original_limit);
        }

        let mut results = if let Some(backend) = backend {
            backend.search(collection, phase1_query).await?
        } else if has_text {
            self.text_backend.search(collection, phase1_query).await?
        } else {
            return Err(Error::Backend(
                "No backend available for collection".to_string(),
            ));
        };

        // Phase 2: Rerank if configured
        if let (Some(config), Some(reranker)) = (rerank_config, reranker) {
            match reranker
                .rerank_results(
                    &query_string_for_rerank,
                    &results.results,
                    &config.text_fields,
                )
                .await
            {
                Ok(new_scores) => {
                    // Apply new scores
                    for (result, &score) in results.results.iter_mut().zip(new_scores.iter()) {
                        result.score = score;
                    }
                    // Re-sort by new scores (highest first)
                    results.results.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    // Truncate back to original limit
                    results.results.truncate(original_limit);

                    tracing::debug!(
                        "Reranked {} candidates down to {} results for collection '{}' using {}",
                        results.total,
                        original_limit,
                        collection,
                        reranker.name()
                    );
                }
                Err(e) => {
                    // Graceful degradation: log warning and return original results
                    tracing::warn!(
                        "Reranking failed for collection '{}': {}. Returning original results.",
                        collection,
                        e
                    );
                    results.results.truncate(original_limit);
                }
            }
        }

        Ok(results)
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

    /// Check if a collection exists
    pub fn collection_exists(&self, name: &str) -> bool {
        self.schemas.read().contains_key(name)
    }

    /// Validate collection name — alphanumeric, hyphens, underscores only
    pub fn validate_collection_name(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(Error::Schema("Collection name cannot be empty".into()));
        }
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(Error::Schema(format!(
                "Collection name '{}' contains invalid characters (use alphanumeric, hyphens, underscores)",
                name
            )));
        }
        Ok(())
    }

    /// Persist schema to disk as YAML (atomic: write .tmp then rename)
    pub fn persist_schema(&self, schema: &CollectionSchema) -> Result<PathBuf> {
        let dir = &self.schemas_dir;
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.yaml", schema.collection));
        let tmp_path = dir.join(format!("{}.yaml.tmp", schema.collection));
        let yaml = serde_yaml::to_string(schema)?;
        std::fs::write(&tmp_path, &yaml)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(path)
    }

    /// Remove schema file from disk
    pub fn remove_schema_file(&self, name: &str) -> Result<()> {
        let path = self.schemas_dir.join(format!("{name}.yaml"));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

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

        // Remove from all manager maps atomically to prevent race conditions
        // where another thread sees a partial removal state
        {
            let mut backends = self.per_collection_backends.write();
            let mut rerankers = self.per_collection_rerankers.write();
            let mut graphs = self.per_collection_graphs.write();
            let mut schemas = self.schemas.write();
            backends.remove(name);
            rerankers.remove(name);
            graphs.remove(name);
            schemas.remove(name);
        }

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
        // Build graph backend if configured
        if let Some(ref graph_config) = schema.backends.graph {
            let graph = ShardedGraphBackend::new(&name, graph_config, self.graph_storage.clone());
            graph.initialize().await?;
            tracing::info!(
                "Initialized graph backend for '{}' ({} shards)",
                name,
                graph.num_shards()
            );
            self.per_collection_graphs
                .write()
                .insert(name.clone(), Arc::new(graph));
        }
        // Build reranker if configured
        if let Some(reranker) = Self::build_reranker_for_schema(&schema) {
            self.per_collection_rerankers
                .write()
                .insert(name.clone(), reranker);
        }
        // Build async rerankers (cross-encoders)
        if let Some(config) = &schema.reranking {
            if config.reranker_type == crate::schema::RerankerType::CrossEncoder {
                let ce_config = config.cross_encoder.as_ref();
                let model_id = ce_config
                    .map(|c| c.model_id.as_str())
                    .unwrap_or("cross-encoder/ms-marco-MiniLM-L-6-v2");
                let max_length = ce_config.map(|c| c.max_length).unwrap_or(512);
                match crate::ranking::CrossEncoderReranker::new(model_id, max_length).await {
                    Ok(reranker) => {
                        self.per_collection_rerankers
                            .write()
                            .insert(name.clone(), Arc::new(reranker));
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to initialize CrossEncoder reranker for '{}': {}",
                            name,
                            e
                        );
                    }
                }
            }
        }
        self.schemas.write().insert(name.clone(), schema.clone());

        // Persist schema to disk so it survives restarts
        let schema_path = self.schemas_dir.join(format!("{}.yaml", name));
        if let Err(e) = std::fs::write(
            &schema_path,
            serde_yaml::to_string(&schema).unwrap_or_default(),
        ) {
            tracing::error!("Failed to persist schema for '{}': {}", name, e);
        }

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

    /// Get the graph backend for a collection, if one is configured.
    pub fn graph_backend(&self, collection: &str) -> Option<Arc<ShardedGraphBackend>> {
        self.per_collection_graphs.read().get(collection).cloned()
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
    #[allow(clippy::too_many_arguments)]
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
                rrf_k: None,
                min_score: None,
                score_function: None,
                skip_ranking: false,
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
                rrf_k: None,
                min_score: None,
                score_function: None,
                skip_ranking: false,
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
            rrf_k: None,
            min_score: None,
            score_function: None,
            skip_ranking: true,
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
            rrf_k: None,
            min_score: None,
            score_function: None,
            skip_ranking: false,
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
    #[allow(clippy::too_many_arguments)]
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

    /// Synchronously merge segments for a collection to reduce search latency.
    pub fn optimize(&self, collection: &str, max_segments: Option<usize>) -> Result<crate::backends::text::OptimizeResult> {
        self.text_backend.optimize(collection, max_segments)
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
                    let result = self.search(&col, q, None).await;
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

/// Resolved reranking configuration after merging schema + request overrides
struct ResolvedRerankConfig {
    candidates: usize,
    text_fields: Vec<String>,
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

    /// Helper to create a basic collection manager with a text-only collection.
    async fn setup_manager(
        temp: &TempDir,
        collection_name: &str,
    ) -> (CollectionManager, PathBuf) {
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&schemas_dir).unwrap();

        fs::write(
            schemas_dir.join(format!("{}.yaml", collection_name)),
            format!(
                r#"
collection: {collection_name}
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
        stored: true
"#
            ),
        )
        .unwrap();

        let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
        let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
        let manager =
            CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap();
        manager.initialize().await.unwrap();

        (manager, schemas_dir)
    }

    fn make_query(q: &str, limit: usize) -> Query {
        Query {
            query_string: q.to_string(),
            fields: vec![],
            limit,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
            rrf_k: None,
            min_score: None,
            score_function: None,
            skip_ranking: false,
        }
    }

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
        let manager = CollectionManager::new(&schemas_dir, text_backend, vector_backend, None)?;
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
            rrf_k: None,
            min_score: None,
            score_function: None,
            skip_ranking: false,
        };

        let results = manager.search("articles", query, None).await?;
        assert!(results.total > 0);

        Ok(())
    }

    // ========================================================================
    // validate_collection_name tests
    // ========================================================================

    #[test]
    fn test_validate_collection_name_valid() {
        assert!(CollectionManager::validate_collection_name("articles").is_ok());
        assert!(CollectionManager::validate_collection_name("my-collection").is_ok());
        assert!(CollectionManager::validate_collection_name("my_collection").is_ok());
        assert!(CollectionManager::validate_collection_name("logs-2026-01").is_ok());
        assert!(CollectionManager::validate_collection_name("a").is_ok());
        assert!(CollectionManager::validate_collection_name("A-B_c-123").is_ok());
    }

    #[test]
    fn test_validate_collection_name_empty() {
        let err = CollectionManager::validate_collection_name("").unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn test_validate_collection_name_special_chars() {
        assert!(CollectionManager::validate_collection_name("has space").is_err());
        assert!(CollectionManager::validate_collection_name("has.dot").is_err());
        assert!(CollectionManager::validate_collection_name("has/slash").is_err());
        assert!(CollectionManager::validate_collection_name("has@at").is_err());
        assert!(CollectionManager::validate_collection_name("has!bang").is_err());
        assert!(CollectionManager::validate_collection_name("has#hash").is_err());
        assert!(CollectionManager::validate_collection_name("../escape").is_err());
    }

    // ========================================================================
    // delete tests
    // ========================================================================

    #[tokio::test]
    async fn test_delete_documents() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        // Index two documents
        manager
            .index(
                "articles",
                vec![
                    Document {
                        id: "d1".to_string(),
                        fields: HashMap::from([("title".into(), json!("First"))]),
                    },
                    Document {
                        id: "d2".to_string(),
                        fields: HashMap::from([("title".into(), json!("Second"))]),
                    },
                ],
            )
            .await?;

        // Delete one document
        manager
            .delete("articles", vec!["d1".to_string()])
            .await?;

        // Verify the deleted document is gone
        let doc = manager.get("articles", "d1").await?;
        assert!(doc.is_none(), "Deleted document should not be found");

        // The other document should still exist
        let doc2 = manager.get("articles", "d2").await?;
        assert!(doc2.is_some(), "Non-deleted document should still exist");

        Ok(())
    }

    #[tokio::test]
    async fn test_delete_nonexistent_collection() {
        let temp = TempDir::new().unwrap();
        let (manager, _) = setup_manager(&temp, "articles").await;

        let result = manager
            .delete("nonexistent", vec!["d1".to_string()])
            .await;
        assert!(result.is_err());
    }

    // ========================================================================
    // stats tests
    // ========================================================================

    #[tokio::test]
    async fn test_stats_empty_collection() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        let stats = manager.stats("articles").await?;
        assert_eq!(stats.document_count, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_stats_after_indexing() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        manager
            .index(
                "articles",
                vec![Document {
                    id: "d1".to_string(),
                    fields: HashMap::from([("title".into(), json!("Test"))]),
                }],
            )
            .await?;

        let stats = manager.stats("articles").await?;
        assert_eq!(stats.document_count, 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_stats_nonexistent_collection() {
        let temp = TempDir::new().unwrap();
        let (manager, _) = setup_manager(&temp, "articles").await;

        let result = manager.stats("nonexistent").await;
        assert!(result.is_err());
    }

    // ========================================================================
    // get tests
    // ========================================================================

    #[tokio::test]
    async fn test_get_existing_document() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        manager
            .index(
                "articles",
                vec![Document {
                    id: "d1".to_string(),
                    fields: HashMap::from([("title".into(), json!("Hello World"))]),
                }],
            )
            .await?;

        let doc = manager.get("articles", "d1").await?;
        assert!(doc.is_some());
        let doc = doc.unwrap();
        assert_eq!(doc.id, "d1");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_missing_document() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        let doc = manager.get("articles", "nonexistent").await?;
        assert!(doc.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn test_get_nonexistent_collection() {
        let temp = TempDir::new().unwrap();
        let (manager, _) = setup_manager(&temp, "articles").await;

        let result = manager.get("nonexistent", "d1").await;
        assert!(result.is_err());
    }

    // ========================================================================
    // search_with_aggs tests
    // ========================================================================

    #[tokio::test]
    async fn test_search_with_aggs_nonexistent_collection() {
        let temp = TempDir::new().unwrap();
        let (manager, _) = setup_manager(&temp, "articles").await;

        let q = make_query("test", 10);
        let result = manager
            .search_with_aggs("nonexistent", &q, vec![])
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_search_with_aggs_empty_aggregations() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        // Index a document
        manager
            .index(
                "articles",
                vec![Document {
                    id: "d1".to_string(),
                    fields: HashMap::from([("title".into(), json!("Test document"))]),
                }],
            )
            .await?;

        let q = make_query("test", 10);
        let result = manager.search_with_aggs("articles", &q, vec![]).await?;
        // With empty agg list, results should still have search results
        assert!(result.results.len() <= result.total as usize);
        Ok(())
    }

    // ========================================================================
    // add_collection / remove_collection / persist_schema / remove_schema_file
    // ========================================================================

    #[tokio::test]
    async fn test_add_and_remove_collection() -> Result<()> {
        let temp = TempDir::new()?;
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&schemas_dir)?;

        let text_backend = Arc::new(TextBackend::new(&data_dir)?);
        let vector_backend = Arc::new(VectorBackend::new(&data_dir)?);
        let manager = CollectionManager::new(&schemas_dir, text_backend, vector_backend, None)?;
        manager.initialize().await?;

        // Initially no collections
        assert!(manager.list_collections().is_empty());

        // Add a collection dynamically
        let schema_yaml = r#"
collection: dynamic
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
"#;
        let schema: crate::schema::CollectionSchema = serde_yaml::from_str(schema_yaml)?;
        manager.add_collection(schema).await?;

        // Verify it was added
        assert!(manager.collection_exists("dynamic"));
        assert_eq!(manager.list_collections().len(), 1);

        // Index a document
        manager
            .index(
                "dynamic",
                vec![Document {
                    id: "d1".to_string(),
                    fields: HashMap::from([("title".into(), json!("Dynamic Doc"))]),
                }],
            )
            .await?;

        let results = manager.search("dynamic", make_query("dynamic", 10), None).await?;
        assert!(results.total > 0);

        // Remove the collection
        manager.remove_collection("dynamic").await?;

        assert!(!manager.collection_exists("dynamic"));
        assert!(manager.list_collections().is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_remove_collection_nonexistent() {
        let temp = TempDir::new().unwrap();
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&schemas_dir).unwrap();

        let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
        let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
        let manager =
            CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap();
        manager.initialize().await.unwrap();

        let result = manager.remove_collection("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_persist_schema_and_remove() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, schemas_dir) = setup_manager(&temp, "articles").await;

        let schema = manager.get_schema("articles").unwrap();

        // Persist the schema
        let path = manager.persist_schema(&schema)?;
        assert!(path.exists());
        assert!(path.to_str().unwrap().ends_with("articles.yaml"));

        // Verify file content is valid YAML
        let content = fs::read_to_string(&path)?;
        let _: crate::schema::CollectionSchema = serde_yaml::from_str(&content)?;

        // Remove the schema file
        manager.remove_schema_file("articles")?;
        assert!(!schemas_dir.join("articles.yaml").exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_remove_schema_file_nonexistent() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        // Removing a schema file that does not exist should succeed (no-op)
        let result = manager.remove_schema_file("nonexistent");
        assert!(result.is_ok());

        Ok(())
    }

    // ========================================================================
    // collection_exists / get_schema / list_collections / lint_schemas
    // ========================================================================

    #[tokio::test]
    async fn test_collection_exists() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        assert!(manager.collection_exists("articles"));
        assert!(!manager.collection_exists("nonexistent"));
        Ok(())
    }

    #[tokio::test]
    async fn test_get_schema() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        let schema = manager.get_schema("articles");
        assert!(schema.is_some());
        assert_eq!(schema.unwrap().collection, "articles");

        let missing = manager.get_schema("nonexistent");
        assert!(missing.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn test_list_collections() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        let collections = manager.list_collections();
        assert_eq!(collections.len(), 1);
        assert!(collections.contains(&"articles".to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn test_lint_schemas_clean() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        let issues = manager.lint_schemas();
        assert!(issues.is_empty(), "Valid schema should have no lint issues");
        Ok(())
    }

    // ========================================================================
    // merge_multi_collection_rrf tests
    // ========================================================================

    #[test]
    fn test_merge_multi_collection_rrf_empty() {
        let result = CollectionManager::merge_multi_collection_rrf(vec![], 60, 10);
        assert_eq!(result.total, 0);
        assert!(result.results.is_empty());
    }

    #[test]
    fn test_merge_multi_collection_rrf_single_collection() {
        let results = SearchResults {
            results: vec![
                crate::backends::SearchResult {
                    id: "d1".to_string(),
                    score: 1.0,
                    fields: HashMap::new(),
                    highlight: None,
                },
                crate::backends::SearchResult {
                    id: "d2".to_string(),
                    score: 0.5,
                    fields: HashMap::new(),
                    highlight: None,
                },
            ],
            total: 2,
            latency_ms: 0,
        };

        let merged = CollectionManager::merge_multi_collection_rrf(
            vec![("col1".to_string(), results)],
            60,
            10,
        );

        assert_eq!(merged.total, 2);
        assert_eq!(merged.results[0].collection, "col1");
        assert_eq!(merged.results[1].collection, "col1");
        // RRF score for rank 1 = 1/(60+1), rank 2 = 1/(60+2)
        assert!(merged.results[0].score > merged.results[1].score);
    }

    #[test]
    fn test_merge_multi_collection_rrf_limit_truncation() {
        let results = SearchResults {
            results: (0..20)
                .map(|i| crate::backends::SearchResult {
                    id: format!("d{}", i),
                    score: 1.0 - (i as f32 * 0.01),
                    fields: HashMap::new(),
                    highlight: None,
                })
                .collect(),
            total: 20,
            latency_ms: 0,
        };

        let merged = CollectionManager::merge_multi_collection_rrf(
            vec![("col1".to_string(), results)],
            60,
            5,
        );

        assert_eq!(merged.results.len(), 5);
        assert_eq!(merged.total, 20);
    }

    // ========================================================================
    // glob_match tests
    // ========================================================================

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

    #[test]
    fn test_glob_match_question_mark() {
        assert!(CollectionManager::glob_match("a?c", "abc"));
        assert!(!CollectionManager::glob_match("a?c", "ac"));
        assert!(!CollectionManager::glob_match("a?c", "abdc"));
    }

    // ========================================================================
    // expand_collection_patterns tests
    // ========================================================================

    #[tokio::test]
    async fn test_expand_collection_patterns_exact() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        let expanded = manager.expand_collection_patterns(&["articles".to_string()]);
        assert_eq!(expanded, vec!["articles".to_string()]);

        // Non-existent exact name should not be returned
        let expanded = manager.expand_collection_patterns(&["nonexistent".to_string()]);
        assert!(expanded.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn test_expand_collection_patterns_no_duplicates() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        let expanded =
            manager.expand_collection_patterns(&["articles".to_string(), "articles".to_string()]);
        assert_eq!(expanded.len(), 1);
        Ok(())
    }

    // ========================================================================
    // multi_search tests
    // ========================================================================

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
        let manager = CollectionManager::new(&schemas_dir, text_backend, vector_backend, None)?;
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
            rrf_k: None,
            min_score: None,
            score_function: None,
            skip_ranking: false,
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
    async fn test_multi_search_empty_patterns() -> Result<()> {
        let temp = TempDir::new()?;
        let (manager, _) = setup_manager(&temp, "articles").await;

        let results = manager
            .multi_search(&["nonexistent-*".to_string()], make_query("test", 10), None)
            .await?;
        assert_eq!(results.total, 0);
        assert!(results.collections_searched.is_empty());
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
        let manager = CollectionManager::new(&schemas_dir, text_backend, vector_backend, None)?;
        manager.initialize().await?;

        // Test pattern expansion
        let expanded = manager.expand_collection_patterns(&["logs-2026-*".to_string()]);
        assert_eq!(expanded.len(), 3);

        // Test non-matching pattern
        let empty = manager.expand_collection_patterns(&["nonexistent-*".to_string()]);
        assert!(empty.is_empty());

        Ok(())
    }

    // ========================================================================
    // resolve_rerank_config tests
    // ========================================================================

    #[test]
    fn test_resolve_rerank_config_none_when_no_config() {
        let schema_yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
"#;
        let schema: crate::schema::CollectionSchema = serde_yaml::from_str(schema_yaml).unwrap();
        let result = CollectionManager::resolve_rerank_config(&schema, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_rerank_config_override_disables() {
        let schema_yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
reranking:
  type: score_function
  score_function: "_score * 2"
  candidates: 50
"#;
        let schema: crate::schema::CollectionSchema = serde_yaml::from_str(schema_yaml).unwrap();
        let override_opts = crate::ranking::reranker::RerankOptions {
            enabled: false,
            candidates: 100,
            text_fields: vec![],
        };
        let result = CollectionManager::resolve_rerank_config(&schema, Some(&override_opts));
        assert!(result.is_none());
    }

    // ========================================================================
    // index error paths
    // ========================================================================

    #[tokio::test]
    async fn test_index_nonexistent_collection() {
        let temp = TempDir::new().unwrap();
        let (manager, _) = setup_manager(&temp, "articles").await;

        let result = manager
            .index(
                "nonexistent",
                vec![Document {
                    id: "d1".to_string(),
                    fields: HashMap::new(),
                }],
            )
            .await;
        assert!(result.is_err());
    }
}
