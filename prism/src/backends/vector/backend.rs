//! Vector search backend with unified SegmentStorage integration
//!
//! All storage (local, S3, cached) goes through the SegmentStorage trait.

use crate::backends::r#trait::{
    BackendStats, Document, Query, SearchBackend, SearchResult, SearchResults,
    SearchResultsWithAggs,
};
use crate::cache::EmbeddingCacheStats;
use crate::error::Result;
use crate::schema::types::CollectionSchema;
use async_trait::async_trait;
use parking_lot::RwLock;
use prism_storage::{Bytes, LocalStorage, SegmentStorage, StoragePath};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tempfile::NamedTempFile;

use super::index::{HnswBackend, HnswIndex, Metric};
use crate::embedding::CachedEmbeddingProvider;

pub struct VectorBackend {
    _base_path: PathBuf,
    indexes: Arc<RwLock<HashMap<String, VectorIndex>>>,
    /// Cached embedding provider for automatic embedding generation
    embedding_provider: Arc<RwLock<Option<Arc<CachedEmbeddingProvider>>>>,
    /// Unified storage backend (local, S3, cached, etc.)
    storage: Arc<dyn SegmentStorage>,
}

struct VectorIndex {
    hnsw: HnswBackend,
    dimensions: usize,
    metric: Metric,
    ef_search: usize,
    // String ID <-> u32 key mapping
    id_to_key: HashMap<String, u32>,
    key_to_id: HashMap<u32, String>,
    next_key: AtomicU32,
    // Store document fields (HNSW only stores vectors)
    documents: HashMap<String, HashMap<String, serde_json::Value>>,
    /// Source field for embedding generation
    embedding_source_field: Option<String>,
    /// Target field for embeddings
    embedding_target_field: String,
}

#[derive(Serialize, Deserialize)]
struct PersistedVectorIndex {
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
    ///
    /// Uses LocalStorage from prism-storage for persistence.
    pub fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        let storage = Arc::new(LocalStorage::new(&base_path));
        Self::with_segment_storage(base_path, storage)
    }

    /// Create a backend with unified SegmentStorage (local, S3, cached, etc.).
    ///
    /// This is the primary constructor - all storage goes through SegmentStorage.
    pub fn with_segment_storage(
        base_path: impl AsRef<Path>,
        storage: Arc<dyn SegmentStorage>,
    ) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        // Ensure local path exists for filesystem-backed stores; harmless for S3
        let _ = std::fs::create_dir_all(&base_path);

        Ok(Self {
            _base_path: base_path,
            indexes: Arc::new(RwLock::new(HashMap::new())),
            embedding_provider: Arc::new(RwLock::new(None)),
            storage,
        })
    }

    /// Set the embedding provider for automatic embedding generation
    pub fn set_embedding_provider(&self, provider: Arc<CachedEmbeddingProvider>) {
        let mut ep = self.embedding_provider.write();
        *ep = Some(provider);
    }

    /// Get cache statistics from the embedding provider
    pub async fn embedding_cache_stats(&self) -> Option<EmbeddingCacheStats> {
        // Clone the provider to avoid holding the lock across await
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
            match deserialize_vector_index(&bytes) {
                Ok(restored) => {
                    let mut indexes = self.indexes.write();
                    indexes.insert(collection.to_string(), restored);
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load persisted vector index, rebuilding");
                }
            }
        }

        let metric = match vector_config.distance {
            crate::schema::types::VectorDistance::Cosine => Metric::Cosine,
            crate::schema::types::VectorDistance::Euclidean => Metric::Euclidean,
            crate::schema::types::VectorDistance::Dot => Metric::DotProduct,
        };

        let hnsw = HnswBackend::new(
            vector_config.dimension,
            metric,
            vector_config.hnsw_m,
            vector_config.hnsw_ef_construction,
        )?;

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

        let vector_index = VectorIndex {
            hnsw,
            dimensions: vector_config.dimension,
            metric,
            ef_search: vector_config.hnsw_ef_search,
            id_to_key: HashMap::new(),
            key_to_id: HashMap::new(),
            next_key: AtomicU32::new(0),
            documents: HashMap::new(),
            embedding_source_field: source_field,
            embedding_target_field: target_field,
        };

        let mut indexes = self.indexes.write();
        indexes.insert(collection.to_string(), vector_index);

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
        // Generate embedding for the query
        let query_vector = self.embed_text(text).await?;

        // Convert to JSON for the standard search
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
        };

        self.search(collection, query).await
    }

    // --- Storage helpers using SegmentStorage ---

    fn index_path(collection: &str) -> StoragePath {
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
        let path = Self::index_path(collection);
        match self.storage.read(&path).await {
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
            let vector_index = indexes
                .get(collection)
                .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;
            vector_index.embedding_source_field.is_some()
        };

        // Auto-embed documents that need it
        if needs_embedding {
            // Clone the provider Arc to avoid holding the guard across await
            let provider = {
                let ep = self.embedding_provider.read();
                ep.clone()
            };

            if let Some(ref provider) = provider {
                let (_source_field, target_field, texts_to_embed) = {
                    let indexes = self.indexes.read();
                    let vector_index = indexes.get(collection).unwrap();
                    let source_field = vector_index.embedding_source_field.clone().unwrap();
                    let target_field = vector_index.embedding_target_field.clone();

                    // Collect texts that need embedding
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
                }; // indexes lock released here

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

        // Index documents with embeddings
        let data = {
            let mut indexes = self.indexes.write();
            let vector_index = indexes
                .get_mut(collection)
                .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

            for doc in docs {
                // Extract vector from document
                let vector_value = doc
                    .fields
                    .get(&vector_index.embedding_target_field)
                    .ok_or_else(|| {
                        crate::error::Error::Schema(format!(
                            "Missing {} field",
                            vector_index.embedding_target_field
                        ))
                    })?;

                let vector: Vec<f32> = serde_json::from_value(vector_value.clone())
                    .map_err(|_| crate::error::Error::Schema("Invalid embedding format".into()))?;

                if vector.len() != vector_index.dimensions {
                    return Err(crate::error::Error::Schema(format!(
                        "Expected {} dimensions, got {}",
                        vector_index.dimensions,
                        vector.len()
                    )));
                }

                let key = vector_index.next_key.fetch_add(1, Ordering::SeqCst);
                vector_index.hnsw.add(key, &vector)?;
                vector_index.id_to_key.insert(doc.id.clone(), key);
                vector_index.key_to_id.insert(key, doc.id.clone());
                vector_index.documents.insert(doc.id.clone(), doc.fields);
            }

            serialize_vector_index(vector_index)?
        };

        self.save_index(collection, &data).await
    }

    async fn search(&self, collection: &str, query: Query) -> Result<SearchResults> {
        let start = std::time::Instant::now();

        let indexes = self.indexes.read();
        let vector_index = indexes
            .get(collection)
            .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

        // Parse query string as vector
        let query_vector: Vec<f32> = serde_json::from_str(&query.query_string)
            .map_err(|_| crate::error::Error::InvalidQuery("Invalid vector format".into()))?;

        if query_vector.len() != vector_index.dimensions {
            return Err(crate::error::Error::InvalidQuery(format!(
                "Expected {} dimensions, got {}",
                vector_index.dimensions,
                query_vector.len()
            )));
        }

        // Search HNSW index
        let matches =
            vector_index
                .hnsw
                .search(&query_vector, query.limit, vector_index.ef_search)?;

        // Convert keys to IDs and retrieve documents
        let mut results = Vec::new();
        for (key, score) in matches {
            if let Some(doc_id) = vector_index.key_to_id.get(&key) {
                if let Some(fields) = vector_index.documents.get(doc_id) {
                    results.push(SearchResult {
                        id: doc_id.clone(),
                        score,
                        fields: fields.clone(),
                        highlight: None,
                    });
                }
            }
        }

        let latency_ms = start.elapsed().as_millis() as u64;

        let total = results.len();
        Ok(SearchResults {
            results,
            total,
            latency_ms,
        })
    }

    async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>> {
        let indexes = self.indexes.read();
        let vector_index = indexes
            .get(collection)
            .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

        if let Some(fields) = vector_index.documents.get(id) {
            Ok(Some(Document {
                id: id.to_string(),
                fields: fields.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<()> {
        let data = {
            let mut indexes = self.indexes.write();
            let vector_index = indexes
                .get_mut(collection)
                .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

            for id in ids {
                if let Some(key) = vector_index.id_to_key.remove(&id) {
                    vector_index.key_to_id.remove(&key);
                    vector_index.hnsw.remove(key)?;
                    vector_index.documents.remove(&id);
                }
            }

            serialize_vector_index(vector_index)?
        };

        self.save_index(collection, &data).await
    }

    async fn stats(&self, collection: &str) -> Result<BackendStats> {
        let indexes = self.indexes.read();
        let vector_index = indexes
            .get(collection)
            .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

        let document_count = vector_index.documents.len();

        // Estimate size
        let vector_size = vector_index.dimensions * 4 * document_count;
        let metadata_size = vector_index
            .documents
            .values()
            .map(|fields| {
                fields
                    .iter()
                    .map(|(k, v)| k.len() + v.to_string().len())
                    .sum::<usize>()
            })
            .sum::<usize>();

        let size_bytes = vector_size + metadata_size;

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
        // VectorBackend doesn't support aggregations yet
        // Return empty aggregations for now
        let results = self.search(collection, query.clone()).await?;

        Ok(SearchResultsWithAggs {
            results: results.results,
            total: results.total as u64,
            aggregations: HashMap::new(),
        })
    }
}

fn serialize_vector_index(vector_index: &VectorIndex) -> Result<Vec<u8>> {
    let tmp = NamedTempFile::new()?;
    vector_index.hnsw.save(tmp.path())?;
    let hnsw_data = fs::read(tmp.path())?;

    let persisted = PersistedVectorIndex {
        dimensions: vector_index.dimensions,
        metric: vector_index.metric,
        ef_search: vector_index.ef_search,
        id_to_key: vector_index.id_to_key.clone(),
        key_to_id: vector_index.key_to_id.clone(),
        next_key: vector_index.next_key.load(Ordering::SeqCst),
        documents: vector_index.documents.clone(),
        embedding_source_field: vector_index.embedding_source_field.clone(),
        embedding_target_field: vector_index.embedding_target_field.clone(),
        hnsw_data,
    };

    Ok(serde_json::to_vec(&persisted)?)
}

fn deserialize_vector_index(bytes: &[u8]) -> Result<VectorIndex> {
    let persisted: PersistedVectorIndex = serde_json::from_slice(bytes)?;
    let tmp = NamedTempFile::new()?;
    fs::write(tmp.path(), &persisted.hnsw_data)?;
    let hnsw = HnswBackend::load(tmp.path())?;

    Ok(VectorIndex {
        hnsw,
        dimensions: persisted.dimensions,
        metric: persisted.metric,
        ef_search: persisted.ef_search,
        id_to_key: persisted.id_to_key,
        key_to_id: persisted.key_to_id,
        next_key: AtomicU32::new(persisted.next_key),
        documents: persisted.documents,
        embedding_source_field: persisted.embedding_source_field,
        embedding_target_field: persisted.embedding_target_field,
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
            // Return deterministic embedding based on text hash
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

        // Create cached embedding provider
        let mock_provider = Box::new(MockEmbeddingProvider { dimensions: 4 });
        let cache = Arc::new(SqliteCache::in_memory().unwrap());
        let cached_provider = Arc::new(CachedEmbeddingProvider::new(
            mock_provider,
            cache,
            KeyStrategy::ModelText,
        ));

        backend.set_embedding_provider(cached_provider);

        // Test embedding
        let embedding = backend.embed_text("hello world").await.unwrap();
        assert_eq!(embedding.len(), 4);
    }

    #[tokio::test]
    async fn test_vector_backend_with_segment_storage() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(LocalStorage::new(dir.path()));
        let backend = VectorBackend::with_segment_storage(dir.path(), storage).unwrap();

        // Verify storage is used correctly
        assert!(backend.load_index("nonexistent").await.unwrap().is_none());
    }
}
