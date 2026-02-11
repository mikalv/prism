//! Vector search backend with sharded HNSW indexes.
//!
//! All storage (local, S3, cached) goes through the SegmentStorage trait.
//! Documents are distributed across shards via hash-based assignment.

use crate::backends::r#trait::{
    BackendStats, Document, Query, SearchBackend, SearchResults,
    SearchResultsWithAggs,
};
use crate::cache::EmbeddingCacheStats;
use crate::error::Result;
use crate::schema::types::{CollectionSchema, VectorCompactionConfig};
use async_trait::async_trait;
use parking_lot::RwLock;
use prism_storage::{Bytes, LocalStorage, SegmentStorage, StoragePath};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use tempfile::NamedTempFile;

use super::compaction::compact_shard;
use super::index::{HnswBackend, HnswIndex, Metric};
use super::segment::VectorSegment;
use super::shard::{shard_for_doc, PersistedShard, VectorShard};
use crate::embedding::CachedEmbeddingProvider;

pub struct VectorBackend {
    _base_path: PathBuf,
    indexes: Arc<RwLock<HashMap<String, ShardedVectorIndex>>>,
    /// Cached embedding provider for automatic embedding generation
    embedding_provider: Arc<RwLock<Option<Arc<CachedEmbeddingProvider>>>>,
    /// Unified storage backend (local, S3, cached, etc.)
    storage: Arc<dyn SegmentStorage>,
}

/// A sharded vector index: holds N shards, each with segments.
struct ShardedVectorIndex {
    shards: Vec<VectorShard>,
    num_shards: usize,
    shard_oversample: f32,
    compaction_config: VectorCompactionConfig,
}

/// Persisted format for a sharded vector index.
#[derive(Serialize, Deserialize)]
struct PersistedShardedIndex {
    num_shards: usize,
    shard_oversample: f32,
    compaction_config: VectorCompactionConfig,
    shards: Vec<PersistedShard>,
}

/// Legacy persisted format (single monolithic index) for backward compatibility.
#[derive(Serialize, Deserialize)]
struct LegacyPersistedVectorIndex {
    dimensions: usize,
    metric: Metric,
    ef_search: usize,
    id_to_key: HashMap<String, u32>,
    key_to_id: HashMap<u32, String>,
    next_key: u32,
    documents: HashMap<String, HashMap<String, serde_json::Value>>,
    embedding_source_field: Option<String>,
    embedding_target_field: String,
    hnsw_data: Vec<u8>,
}

impl VectorBackend {
    /// Create a new VectorBackend with local filesystem storage.
    pub fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        let storage = Arc::new(LocalStorage::new(&base_path));
        Self::with_segment_storage(base_path, storage)
    }

    /// Create a backend with unified SegmentStorage (local, S3, cached, etc.).
    pub fn with_segment_storage(
        base_path: impl AsRef<Path>,
        storage: Arc<dyn SegmentStorage>,
    ) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        let _ = std::fs::create_dir_all(&base_path);

        Ok(Self {
            _base_path: base_path,
            indexes: Arc::new(RwLock::new(HashMap::new())),
            embedding_provider: Arc::new(RwLock::new(None)),
            storage,
        })
    }

    /// Remove a collection from this backend, persisting state before dropping.
    pub async fn remove_collection(&self, name: &str) -> Result<()> {
        let data = {
            let mut indexes = self.indexes.write();
            if let Some(index) = indexes.get(name) {
                let data = serialize_sharded_index(index)?;
                indexes.remove(name);
                Some(data)
            } else {
                None
            }
        };
        if let Some(data) = data {
            self.save_index(name, &data).await?;
        }
        Ok(())
    }

    /// Set the embedding provider for automatic embedding generation
    pub fn set_embedding_provider(&self, provider: Arc<CachedEmbeddingProvider>) {
        let mut ep = self.embedding_provider.write();
        *ep = Some(provider);
    }

    /// Get cache statistics from the embedding provider
    pub async fn embedding_cache_stats(&self) -> Option<EmbeddingCacheStats> {
        let provider = {
            let ep = self.embedding_provider.read();
            ep.clone()
        };

        if let Some(provider) = provider {
            provider.cache_stats().await.ok()
        } else {
            None
        }
    }

    pub async fn initialize(&self, collection: &str, schema: &CollectionSchema) -> Result<()> {
        let vector_config =
            schema.backends.vector.as_ref().ok_or_else(|| {
                crate::error::Error::Schema("No vector backend configured".into())
            })?;

        // Attempt to restore from persistence first
        if let Some(bytes) = self.load_index(collection).await? {
            // Try new sharded format first, then legacy format
            match deserialize_sharded_index(&bytes) {
                Ok(restored) => {
                    let mut indexes = self.indexes.write();
                    indexes.insert(collection.to_string(), restored);
                    return Ok(());
                }
                Err(_) => {
                    // Try legacy format
                    match deserialize_legacy_index(&bytes, vector_config) {
                        Ok(restored) => {
                            tracing::info!(
                                collection,
                                "Migrated legacy vector index to sharded format"
                            );
                            let mut indexes = self.indexes.write();
                            indexes.insert(collection.to_string(), restored);
                            return Ok(());
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to load persisted vector index, rebuilding");
                        }
                    }
                }
            }
        }

        let metric = match vector_config.distance {
            crate::schema::types::VectorDistance::Cosine => Metric::Cosine,
            crate::schema::types::VectorDistance::Euclidean => Metric::Euclidean,
            crate::schema::types::VectorDistance::Dot => Metric::DotProduct,
        };

        // Get embedding config if available
        let (source_field, target_field) = if let Some(ref emb_cfg) = schema.embedding_generation {
            if emb_cfg.enabled {
                (
                    Some(emb_cfg.source_field.clone()),
                    emb_cfg.target_field.clone(),
                )
            } else {
                (None, "embedding".to_string())
            }
        } else {
            (None, "embedding".to_string())
        };

        let num_shards = vector_config.num_shards.max(1);
        let mut shards = Vec::with_capacity(num_shards);
        for i in 0..num_shards {
            let shard = VectorShard::new(
                i as u32,
                vector_config.dimension,
                metric,
                vector_config.hnsw_m,
                vector_config.hnsw_ef_construction,
                vector_config.hnsw_ef_search,
                source_field.clone(),
                target_field.clone(),
            )?;
            shards.push(shard);
        }

        let sharded = ShardedVectorIndex {
            shards,
            num_shards,
            shard_oversample: vector_config.shard_oversample,
            compaction_config: vector_config.compaction.clone(),
        };

        let mut indexes = self.indexes.write();
        indexes.insert(collection.to_string(), sharded);

        Ok(())
    }

    /// Embed a single text using the cached provider
    #[tracing::instrument(name = "embed_text", skip(self, text))]
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let start = std::time::Instant::now();
        let ep = self.embedding_provider.read();
        if let Some(ref provider) = *ep {
            let result = provider
                .embed(text)
                .await
                .map_err(|e| crate::error::Error::Backend(format!("Embedding failed: {}", e)));

            let duration = start.elapsed().as_secs_f64();
            let status = if result.is_ok() { "ok" } else { "error" };
            metrics::histogram!("prism_embedding_duration_seconds", "provider" => "ort")
                .record(duration);
            metrics::counter!("prism_embedding_requests_total", "provider" => "ort", "status" => status)
                .increment(1);

            result
        } else {
            Err(crate::error::Error::Backend(
                "No embedding provider configured. Call set_embedding_provider() first.".into(),
            ))
        }
    }

    /// Embed multiple texts using the cached provider (uses batch API)
    #[tracing::instrument(name = "embed_texts", skip(self, texts), fields(text_count = texts.len()))]
    pub async fn embed_texts(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let start = std::time::Instant::now();
        let ep = self.embedding_provider.read();
        if let Some(ref provider) = *ep {
            let result = provider.embed_batch(texts).await.map_err(|e| {
                crate::error::Error::Backend(format!("Batch embedding failed: {}", e))
            });

            let duration = start.elapsed().as_secs_f64();
            let status = if result.is_ok() { "ok" } else { "error" };
            metrics::histogram!("prism_embedding_duration_seconds", "provider" => "ort")
                .record(duration);
            metrics::counter!("prism_embedding_requests_total", "provider" => "ort", "status" => status)
                .increment(texts.len() as u64);

            result
        } else {
            Err(crate::error::Error::Backend(
                "No embedding provider configured. Call set_embedding_provider() first.".into(),
            ))
        }
    }

    /// Search with a text query (auto-embeds the query)
    #[tracing::instrument(name = "vector_search", skip(self, text), fields(collection = %collection))]
    pub async fn search_text(
        &self,
        collection: &str,
        text: &str,
        limit: usize,
    ) -> Result<SearchResults> {
        let query_vector = self.embed_text(text).await?;

        let query = Query {
            query_string: serde_json::to_string(&query_vector)
                .map_err(|e| crate::error::Error::Backend(format!("JSON error: {}", e)))?,
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

        self.search(collection, query).await
    }

    // --- Storage helpers using SegmentStorage ---

    fn index_path(collection: &str) -> StoragePath {
        StoragePath::vector(collection, "default", "sharded_index.json")
    }

    fn legacy_index_path(collection: &str) -> StoragePath {
        StoragePath::vector(collection, "default", "vector_index.json")
    }

    async fn save_index(&self, collection: &str, data: &[u8]) -> Result<()> {
        let path = Self::index_path(collection);
        self.storage
            .write(&path, Bytes::copy_from_slice(data))
            .await
            .map_err(|e| crate::error::Error::Storage(e.to_string()))
    }

    async fn load_index(&self, collection: &str) -> Result<Option<Vec<u8>>> {
        // Try new format first
        let path = Self::index_path(collection);
        match self.storage.read(&path).await {
            Ok(data) => return Ok(Some(data.to_vec())),
            Err(prism_storage::StorageError::NotFound(_)) => {}
            Err(e) => return Err(crate::error::Error::Storage(e.to_string())),
        }

        // Fall back to legacy format
        let legacy_path = Self::legacy_index_path(collection);
        match self.storage.read(&legacy_path).await {
            Ok(data) => Ok(Some(data.to_vec())),
            Err(prism_storage::StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(crate::error::Error::Storage(e.to_string())),
        }
    }
}

#[async_trait]
impl SearchBackend for VectorBackend {
    async fn index(&self, collection: &str, mut docs: Vec<Document>) -> Result<()> {
        // Check if we need auto-embedding
        let needs_embedding = {
            let indexes = self.indexes.read();
            let sharded = indexes
                .get(collection)
                .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;
            sharded
                .shards
                .first()
                .map(|s| s.embedding_source_field.is_some())
                .unwrap_or(false)
        };

        // Auto-embed documents that need it
        if needs_embedding {
            let provider = {
                let ep = self.embedding_provider.read();
                ep.clone()
            };

            if let Some(ref provider) = provider {
                let (_source_field, target_field, texts_to_embed) = {
                    let indexes = self.indexes.read();
                    let sharded = indexes.get(collection).unwrap();
                    let first_shard = &sharded.shards[0];
                    let source_field = first_shard.embedding_source_field.clone().unwrap();
                    let target_field = first_shard.embedding_target_field.clone();

                    let mut texts_to_embed: Vec<(usize, String)> = Vec::new();
                    for (i, doc) in docs.iter().enumerate() {
                        if !doc.fields.contains_key(&target_field) {
                            if let Some(val) = doc.fields.get(&source_field) {
                                if let Some(s) = val.as_str() {
                                    texts_to_embed.push((i, s.to_string()));
                                }
                            }
                        }
                    }
                    (source_field, target_field, texts_to_embed)
                };

                if !texts_to_embed.is_empty() {
                    tracing::info!(
                        "Auto-generating {} embeddings (with cache)",
                        texts_to_embed.len()
                    );
                    let texts: Vec<&str> = texts_to_embed.iter().map(|(_, s)| s.as_str()).collect();

                    match provider.embed_batch(&texts).await {
                        Ok(embeddings) => {
                            for ((doc_idx, _), embedding) in
                                texts_to_embed.iter().zip(embeddings.into_iter())
                            {
                                docs[*doc_idx].fields.insert(
                                    target_field.clone(),
                                    serde_json::to_value(&embedding).unwrap(),
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!("Embedding generation failed: {}", e);
                        }
                    }
                }
            }
        }

        // Index documents with embeddings, routing to shards by doc ID hash
        let data = {
            let mut indexes = self.indexes.write();
            let sharded = indexes
                .get_mut(collection)
                .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

            let target_field = sharded
                .shards
                .first()
                .map(|s| s.embedding_target_field.clone())
                .unwrap_or_else(|| "embedding".to_string());

            let dimensions = sharded
                .shards
                .first()
                .map(|s| s.dimensions)
                .unwrap_or(0);

            for doc in docs {
                let vector_value = doc.fields.get(&target_field).ok_or_else(|| {
                    crate::error::Error::Schema(format!("Missing {} field", target_field))
                })?;

                let vector: Vec<f32> = serde_json::from_value(vector_value.clone())
                    .map_err(|_| crate::error::Error::Schema("Invalid embedding format".into()))?;

                if vector.len() != dimensions {
                    return Err(crate::error::Error::Schema(format!(
                        "Expected {} dimensions, got {}",
                        dimensions,
                        vector.len()
                    )));
                }

                let shard_id = shard_for_doc(&doc.id, sharded.num_shards) as usize;
                sharded.shards[shard_id].index(&doc.id, &vector, doc.fields)?;
            }

            serialize_sharded_index(sharded)?
        };

        self.save_index(collection, &data).await
    }

    async fn search(&self, collection: &str, query: Query) -> Result<SearchResults> {
        let start = std::time::Instant::now();

        let indexes = self.indexes.read();
        let sharded = indexes
            .get(collection)
            .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

        let query_vector: Vec<f32> = serde_json::from_str(&query.query_string)
            .map_err(|_| crate::error::Error::InvalidQuery("Invalid vector format".into()))?;

        let dimensions = sharded
            .shards
            .first()
            .map(|s| s.dimensions)
            .unwrap_or(0);

        if query_vector.len() != dimensions {
            return Err(crate::error::Error::InvalidQuery(format!(
                "Expected {} dimensions, got {}",
                dimensions,
                query_vector.len()
            )));
        }

        // Fan out search to all shards with oversample factor
        let oversample_k = if sharded.num_shards > 1 {
            ((query.limit as f32) * sharded.shard_oversample).ceil() as usize
        } else {
            query.limit
        };

        let mut all_results = Vec::new();
        for shard in &sharded.shards {
            let shard_results = shard.search(&query_vector, oversample_k)?;
            all_results.extend(shard_results);
        }

        // Merge results by score (descending), take top-k
        all_results
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Dedup by id
        let mut seen = std::collections::HashSet::new();
        all_results.retain(|r| seen.insert(r.id.clone()));

        all_results.truncate(query.limit);

        let latency_ms = start.elapsed().as_millis() as u64;
        let total = all_results.len();

        Ok(SearchResults {
            results: all_results,
            total,
            latency_ms,
        })
    }

    async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>> {
        let indexes = self.indexes.read();
        let sharded = indexes
            .get(collection)
            .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

        // Route to correct shard
        let shard_id = shard_for_doc(id, sharded.num_shards) as usize;
        if let Some(fields) = sharded.shards[shard_id].get(id) {
            Ok(Some(Document {
                id: id.to_string(),
                fields,
            }))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<()> {
        let data = {
            let mut indexes = self.indexes.write();
            let sharded = indexes
                .get_mut(collection)
                .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

            for id in &ids {
                let shard_id = shard_for_doc(id, sharded.num_shards) as usize;
                sharded.shards[shard_id].delete(id);
            }

            // Check if any shard needs compaction
            let config = sharded.compaction_config.clone();
            for shard in &mut sharded.shards {
                let _ = compact_shard(shard, &config);
            }

            serialize_sharded_index(sharded)?
        };

        self.save_index(collection, &data).await
    }

    async fn stats(&self, collection: &str) -> Result<BackendStats> {
        let indexes = self.indexes.read();
        let sharded = indexes
            .get(collection)
            .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

        let dimensions = sharded
            .shards
            .first()
            .map(|s| s.dimensions)
            .unwrap_or(0);

        let document_count: usize = sharded.shards.iter().map(|s| s.live_count() as usize).sum();
        let size_bytes: usize = sharded
            .shards
            .iter()
            .map(|s| s.estimated_size(dimensions))
            .sum();

        Ok(BackendStats {
            document_count,
            size_bytes,
        })
    }

    async fn search_with_aggs(
        &self,
        collection: &str,
        query: &Query,
        _aggregations: Vec<crate::aggregations::AggregationRequest>,
    ) -> Result<SearchResultsWithAggs> {
        let results = self.search(collection, query.clone()).await?;

        Ok(SearchResultsWithAggs {
            results: results.results,
            total: results.total as u64,
            aggregations: HashMap::new(),
        })
    }
}

fn serialize_sharded_index(index: &ShardedVectorIndex) -> Result<Vec<u8>> {
    let mut persisted_shards = Vec::new();
    for shard in &index.shards {
        persisted_shards.push(shard.to_persisted()?);
    }

    let persisted = PersistedShardedIndex {
        num_shards: index.num_shards,
        shard_oversample: index.shard_oversample,
        compaction_config: index.compaction_config.clone(),
        shards: persisted_shards,
    };

    Ok(serde_json::to_vec(&persisted)?)
}

fn deserialize_sharded_index(bytes: &[u8]) -> Result<ShardedVectorIndex> {
    let persisted: PersistedShardedIndex = serde_json::from_slice(bytes)?;
    let mut shards = Vec::new();
    for shard_p in persisted.shards {
        shards.push(VectorShard::from_persisted(shard_p)?);
    }
    Ok(ShardedVectorIndex {
        shards,
        num_shards: persisted.num_shards,
        shard_oversample: persisted.shard_oversample,
        compaction_config: persisted.compaction_config,
    })
}

/// Migrate a legacy single-index format to a single-shard ShardedVectorIndex.
fn deserialize_legacy_index(
    bytes: &[u8],
    vector_config: &crate::schema::types::VectorBackendConfig,
) -> Result<ShardedVectorIndex> {
    let legacy: LegacyPersistedVectorIndex = serde_json::from_slice(bytes)?;

    let tmp = NamedTempFile::new()?;
    std::fs::write(tmp.path(), &legacy.hnsw_data)?;
    let hnsw = HnswBackend::load(tmp.path())?;

    // Build a VectorSegment from the legacy data
    let segment = VectorSegment {
        id: 0,
        hnsw,
        tombstones: roaring::RoaringBitmap::new(),
        id_to_key: legacy.id_to_key,
        key_to_id: legacy.key_to_id,
        next_key: AtomicU32::new(legacy.next_key),
        documents: legacy.documents,
        dimensions: legacy.dimensions,
        metric: legacy.metric,
        sealed: false,
    };

    let shard = VectorShard {
        shard_id: 0,
        active_segment: segment,
        sealed_segments: Vec::new(),
        dimensions: legacy.dimensions,
        metric: legacy.metric,
        m: vector_config.hnsw_m,
        ef_construction: vector_config.hnsw_ef_construction,
        ef_search: legacy.ef_search,
        embedding_source_field: legacy.embedding_source_field,
        embedding_target_field: legacy.embedding_target_field,
        next_segment_id: 1,
    };

    Ok(ShardedVectorIndex {
        shards: vec![shard],
        num_shards: 1,
        shard_oversample: vector_config.shard_oversample,
        compaction_config: vector_config.compaction.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{KeyStrategy, SqliteCache};
    use crate::embedding::{CachedEmbeddingProvider, EmbeddingProvider};
    use tempfile::tempdir;

    struct MockEmbeddingProvider {
        dimensions: usize,
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for MockEmbeddingProvider {
        async fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
            Ok(vec![0.1, 0.2, 0.3, 0.4])
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        fn dimensions(&self) -> usize {
            self.dimensions
        }
    }

    #[tokio::test]
    async fn test_vector_backend_with_cached_provider() {
        let dir = tempdir().unwrap();
        let backend = VectorBackend::new(dir.path()).unwrap();

        let mock_provider = Box::new(MockEmbeddingProvider { dimensions: 4 });
        let cache = Arc::new(SqliteCache::in_memory().unwrap());
        let cached_provider = Arc::new(CachedEmbeddingProvider::new(
            mock_provider,
            cache,
            KeyStrategy::ModelText,
        ));

        backend.set_embedding_provider(cached_provider);

        let embedding = backend.embed_text("hello world").await.unwrap();
        assert_eq!(embedding.len(), 4);
    }

    #[tokio::test]
    async fn test_vector_backend_with_segment_storage() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(LocalStorage::new(dir.path()));
        let backend = VectorBackend::with_segment_storage(dir.path(), storage).unwrap();

        assert!(backend.load_index("nonexistent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_sharded_index_basic() {
        let dir = tempdir().unwrap();
        let backend = VectorBackend::new(dir.path()).unwrap();

        let schema = make_test_schema(4, 4);
        backend.initialize("test", &schema).await.unwrap();

        // Index some documents
        let docs = vec![
            Document {
                id: "doc1".to_string(),
                fields: {
                    let mut f = HashMap::new();
                    f.insert(
                        "embedding".to_string(),
                        serde_json::to_value(vec![1.0f32, 0.0, 0.0, 0.0]).unwrap(),
                    );
                    f
                },
            },
            Document {
                id: "doc2".to_string(),
                fields: {
                    let mut f = HashMap::new();
                    f.insert(
                        "embedding".to_string(),
                        serde_json::to_value(vec![0.0f32, 1.0, 0.0, 0.0]).unwrap(),
                    );
                    f
                },
            },
        ];
        backend.index("test", docs).await.unwrap();

        // Search
        let query = Query {
            query_string: serde_json::to_string(&vec![1.0f32, 0.0, 0.0, 0.0]).unwrap(),
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
        let results = backend.search("test", query).await.unwrap();
        assert!(!results.results.is_empty());

        // Get
        let doc = backend.get("test", "doc1").await.unwrap();
        assert!(doc.is_some());

        // Delete
        backend
            .delete("test", vec!["doc1".to_string()])
            .await
            .unwrap();
        let doc = backend.get("test", "doc1").await.unwrap();
        assert!(doc.is_none());
    }

    #[tokio::test]
    async fn test_multi_shard_distribution() {
        let dir = tempdir().unwrap();
        let backend = VectorBackend::new(dir.path()).unwrap();

        let schema = make_test_schema(4, 4);
        backend.initialize("test", &schema).await.unwrap();

        // Index 100 documents
        let mut docs = Vec::new();
        for i in 0..100 {
            let vec = vec![
                (i as f32) / 100.0,
                ((100 - i) as f32) / 100.0,
                0.5,
                0.5,
            ];
            docs.push(Document {
                id: format!("doc_{}", i),
                fields: {
                    let mut f = HashMap::new();
                    f.insert("embedding".to_string(), serde_json::to_value(&vec).unwrap());
                    f
                },
            });
        }
        backend.index("test", docs).await.unwrap();

        // Verify all docs are retrievable
        for i in 0..100 {
            let doc = backend
                .get("test", &format!("doc_{}", i))
                .await
                .unwrap();
            assert!(doc.is_some(), "doc_{} not found", i);
        }

        // Verify stats
        let stats = backend.stats("test").await.unwrap();
        assert_eq!(stats.document_count, 100);
    }

    #[tokio::test]
    async fn test_single_shard_backward_compat() {
        let dir = tempdir().unwrap();
        let backend = VectorBackend::new(dir.path()).unwrap();

        // Schema with num_shards: 1 (default)
        let schema = make_test_schema(1, 4);
        backend.initialize("test", &schema).await.unwrap();

        let docs = vec![Document {
            id: "doc1".to_string(),
            fields: {
                let mut f = HashMap::new();
                f.insert(
                    "embedding".to_string(),
                    serde_json::to_value(vec![1.0f32, 0.0, 0.0, 0.0]).unwrap(),
                );
                f
            },
        }];
        backend.index("test", docs).await.unwrap();

        let query = Query {
            query_string: serde_json::to_string(&vec![1.0f32, 0.0, 0.0, 0.0]).unwrap(),
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
        let results = backend.search("test", query).await.unwrap();
        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].id, "doc1");
    }

    #[tokio::test]
    async fn test_persistence_roundtrip() {
        let dir = tempdir().unwrap();

        // Index docs
        {
            let backend = VectorBackend::new(dir.path()).unwrap();
            let schema = make_test_schema(2, 4);
            backend.initialize("test", &schema).await.unwrap();

            let docs = vec![
                Document {
                    id: "doc1".to_string(),
                    fields: {
                        let mut f = HashMap::new();
                        f.insert(
                            "embedding".to_string(),
                            serde_json::to_value(vec![1.0f32, 0.0, 0.0, 0.0]).unwrap(),
                        );
                        f
                    },
                },
                Document {
                    id: "doc2".to_string(),
                    fields: {
                        let mut f = HashMap::new();
                        f.insert(
                            "embedding".to_string(),
                            serde_json::to_value(vec![0.0f32, 1.0, 0.0, 0.0]).unwrap(),
                        );
                        f
                    },
                },
            ];
            backend.index("test", docs).await.unwrap();
        }

        // Reload and verify
        {
            let backend = VectorBackend::new(dir.path()).unwrap();
            let schema = make_test_schema(2, 4);
            backend.initialize("test", &schema).await.unwrap();

            let doc1 = backend.get("test", "doc1").await.unwrap();
            assert!(doc1.is_some());
            let doc2 = backend.get("test", "doc2").await.unwrap();
            assert!(doc2.is_some());
        }
    }

    fn make_test_schema(
        num_shards: usize,
        dimension: usize,
    ) -> CollectionSchema {
        use crate::schema::types::*;
        use crate::storage::StorageConfig;

        CollectionSchema {
            collection: "test".to_string(),
            description: None,
            backends: Backends {
                text: None,
                vector: Some(VectorBackendConfig {
                    embedding_field: "embedding".to_string(),
                    dimension,
                    distance: VectorDistance::Cosine,
                    hnsw_m: 16,
                    hnsw_ef_construction: 200,
                    hnsw_ef_search: 100,
                    vector_weight: 0.5,
                    num_shards,
                    shard_oversample: 2.5,
                    compaction: VectorCompactionConfig::default(),
                }),
                graph: None,
            },
            indexing: IndexingConfig::default(),
            quota: QuotaConfig::default(),
            embedding_generation: None,
            facets: None,
            boosting: None,
            storage: StorageConfig::default(),
            system_fields: SystemFieldsConfig::default(),
            hybrid: None,
            replication: None,
            reranking: None,
            ilm_policy: None,
        }
    }
}
