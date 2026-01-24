use crate::backends::r#trait::{BackendStats, Document, Query, SearchBackend, SearchResult, SearchResults};
use crate::error::Result;
use crate::schema::types::CollectionSchema;
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use super::index::{HnswBackend, HnswIndex, Metric};

pub struct VectorBackend {
    base_path: PathBuf,
    indexes: Arc<RwLock<HashMap<String, VectorIndex>>>,
    #[cfg(feature = "embedding-gen")]
    embedder: Arc<RwLock<Option<Arc<crate::embedding::Embedder>>>>,
}

struct VectorIndex {
    hnsw: HnswBackend,  // Compile-time selected backend
    dimensions: usize,
    metric: Metric,
    ef_search: usize,
    // String ID <-> u32 key mapping
    id_to_key: HashMap<String, u32>,
    key_to_id: HashMap<u32, String>,
    next_key: AtomicU32,
    // Store document fields (HNSW only stores vectors)
    documents: HashMap<String, HashMap<String, serde_json::Value>>,
    #[cfg(feature = "embedding-gen")]
    embedding_generation: Option<crate::schema::types::EmbeddingGenerationConfig>,
}

impl VectorBackend {
    pub fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_path)?;

        Ok(Self {
            base_path,
            indexes: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "embedding-gen")]
            embedder: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn initialize(&self, collection: &str, schema: &CollectionSchema) -> Result<()> {
        let vector_config = schema
            .backends
            .vector
            .as_ref()
            .ok_or_else(|| crate::error::Error::Schema("No vector backend configured".into()))?;

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

        let vector_index = VectorIndex {
            hnsw,
            dimensions: vector_config.dimension,
            metric,
            ef_search: vector_config.hnsw_ef_search,
            id_to_key: HashMap::new(),
            key_to_id: HashMap::new(),
            next_key: AtomicU32::new(0),
            documents: HashMap::new(),
            #[cfg(feature = "embedding-gen")]
            embedding_generation: schema.embedding_generation.clone(),
        };

        let mut indexes = self.indexes.write();
        indexes.insert(collection.to_string(), vector_index);

        #[cfg(feature = "embedding-gen")]
        {
            // If collection schema requests embedding generation, attempt to instantiate embedder
            if let Some(emb_cfg) = &schema.embedding_generation {
                if emb_cfg.enabled {
                    // model override from schema or default
                    let model_name = if emb_cfg.model.is_empty() { "all-MiniLM-L6-v2".to_string() } else { emb_cfg.model.clone() };
                    let config = crate::embedding::ModelConfig::new(model_name);
                    match crate::embedding::Embedder::new(config).await {
                        Ok(embedder) => {
                            let mut e = self.embedder.write();
                            *e = Some(Arc::new(embedder));
                            tracing::info!("Embedder initialized for collection {}", collection);
                        }
                        Err(err) => {
                            tracing::warn!("Failed to initialize embedder for {}: {}. Falling back to deterministic.", collection, err);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl SearchBackend for VectorBackend {
    async fn index(&self, collection: &str, mut docs: Vec<Document>) -> Result<()> {
        // Auto-embedding (feature gated)
        #[cfg(feature = "embedding-gen")]
        {
            let mut indexes = self.indexes.write();
            let vector_index = indexes
                .get_mut(collection)
                .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

            if let Some(cfg) = &vector_index.embedding_generation {
                if cfg.enabled {
                    let embedder_opt = self.embedder.read();
                    if let Some(embedder_arc) = embedder_opt.as_ref() {
                        let embedder = embedder_arc.clone();
                        let mut texts: Vec<&str> = Vec::new();
                        let mut need_idx: Vec<usize> = Vec::new();
                        for (i, doc) in docs.iter().enumerate() {
                            if !doc.fields.contains_key(&cfg.target_field) {
                                if let Some(val) = doc.fields.get(&cfg.source_field) {
                                    if let Some(s) = val.as_str() {
                                        texts.push(s);
                                        need_idx.push(i);
                                    }
                                }
                            }
                        }

                        if !texts.is_empty() {
                            tracing::info!("Auto-generating {} embeddings", texts.len());
                            match embedder.embed_batch(&texts) {
                                Ok(embs) => {
                                    for (emb, &doc_i) in embs.iter().zip(need_idx.iter()) {
                                        docs[doc_i].fields.insert(cfg.target_field.clone(), serde_json::to_value(emb).unwrap());
                                    }
                                }
                                Err(e) => tracing::error!("Embedding generation failed: {}", e),
                            }
                        }
                    }
                }
            }
        }

        let mut indexes = self.indexes.write();
        let vector_index = indexes
            .get_mut(collection)
            .ok_or_else(|| crate::error::Error::CollectionNotFound(collection.to_string()))?;

        for doc in docs {
            // Extract vector from document (assume field name "embedding")
            let vector_value = doc
                .fields
                .get("embedding")
                .ok_or_else(|| crate::error::Error::Schema("Missing embedding field".into()))?;

            let vector: Vec<f32> = serde_json::from_value(vector_value.clone())
                .map_err(|_| crate::error::Error::Schema("Invalid embedding format".into()))?;

            if vector.len() != vector_index.dimensions {
                return Err(crate::error::Error::Schema(format!(
                    "Expected {} dimensions, got {}",
                    vector_index.dimensions,
                    vector.len()
                )));
            }

            // Allocate new key
            let key = vector_index.next_key.fetch_add(1, Ordering::SeqCst);

            // Add to HNSW index
            vector_index.hnsw.add(key, &vector)?;

            // Store ID mappings
            vector_index.id_to_key.insert(doc.id.clone(), key);
            vector_index.key_to_id.insert(key, doc.id.clone());

            // Store document fields
            vector_index.documents.insert(doc.id.clone(), doc.fields);
        }

        Ok(())
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
        let matches = vector_index
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

        Ok(())
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
}

impl VectorBackend {
    /// Generate embedding for a query text using the configured embedder.
    /// Returns the embedding vector if embedding generation is enabled and available.
    #[cfg(feature = "embedding-gen")]
    pub fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let embedder_guard = self.embedder.read();
        if let Some(ref embedder) = *embedder_guard {
            embedder
                .embed(text)
                .map_err(|e| crate::error::Error::Backend(format!("Embedding failed: {}", e)))
        } else {
            Err(crate::error::Error::Backend(
                "No embedder available. Enable embedding-gen feature and configure embedding_generation in schema.".into(),
            ))
        }
    }

    /// Stub for when embedding-gen feature is disabled
    #[cfg(not(feature = "embedding-gen"))]
    pub fn embed_query(&self, _text: &str) -> Result<Vec<f32>> {
        Err(crate::error::Error::Backend(
            "Embedding generation not enabled. Compile with --features embedding-gen".into(),
        ))
    }
}
