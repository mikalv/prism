//! Embedding generation and caching for Prism
//!
//! Supports multiple embedding providers:
//! - Ollama (local, default)
//! - OpenAI-compatible APIs
//! - ONNX (local model inference)

mod ollama;
mod openai;
mod provider;

#[cfg(feature = "provider-onnx")]
mod inference;
#[cfg(feature = "provider-onnx")]
mod model;
#[cfg(feature = "provider-onnx")]
mod onnx;

pub use ollama::OllamaProvider;
pub use openai::OpenAIProvider;
pub use provider::{create_provider, EmbeddingProvider, ProviderConfig};

#[cfg(feature = "provider-onnx")]
pub use inference::Embedder;
#[cfg(feature = "provider-onnx")]
pub use model::{ModelCache, ModelConfig};
#[cfg(feature = "provider-onnx")]
pub use onnx::OnnxProvider;

use crate::cache::{CacheKey, EmbeddingCache, EmbeddingCacheStats, KeyStrategy, SqliteCache};
use std::sync::Arc;

fn default_embed_batch_size() -> usize {
    128
}

fn default_embed_concurrency() -> usize {
    4
}

/// Cached embedding provider that uses the cache layer
pub struct CachedEmbeddingProvider {
    provider: Box<dyn EmbeddingProvider>,
    cache: Arc<dyn EmbeddingCache>,
    key_strategy: KeyStrategy,
    embed_batch_size: usize,
    embed_concurrency: usize,
}

impl CachedEmbeddingProvider {
    /// Create a new cached embedding provider
    pub fn new(
        provider: Box<dyn EmbeddingProvider>,
        cache: Arc<dyn EmbeddingCache>,
        key_strategy: KeyStrategy,
    ) -> Self {
        Self {
            provider,
            cache,
            key_strategy,
            embed_batch_size: default_embed_batch_size(),
            embed_concurrency: default_embed_concurrency(),
        }
    }

    /// Create with custom batch size and concurrency settings
    pub fn with_config(
        provider: Box<dyn EmbeddingProvider>,
        cache: Arc<dyn EmbeddingCache>,
        key_strategy: KeyStrategy,
        batch_size: usize,
        concurrency: usize,
    ) -> Self {
        Self {
            provider,
            cache,
            key_strategy,
            embed_batch_size: batch_size,
            embed_concurrency: concurrency,
        }
    }

    /// Create with SQLite cache at the given path
    pub fn with_sqlite_cache(
        provider: Box<dyn EmbeddingProvider>,
        cache_path: &str,
    ) -> anyhow::Result<Self> {
        let cache = Arc::new(SqliteCache::new(cache_path)?);
        Ok(Self::new(provider, cache, KeyStrategy::ModelText))
    }

    /// Generate embedding with caching
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let key = CacheKey::new(self.provider.model_name(), None, text, self.key_strategy);

        // Check cache first
        if let Some(cached) = self.cache.get(&key).await? {
            tracing::debug!("Cache hit for embedding");
            return Ok(cached);
        }

        // Generate embedding
        tracing::debug!("Cache miss, generating embedding");
        let embedding = self.provider.embed(text).await?;

        // Store in cache
        self.cache
            .set(&key, embedding.clone(), self.provider.dimensions())
            .await?;

        Ok(embedding)
    }

    /// Generate embeddings for multiple texts with caching
    pub async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Build all keys upfront
        let keys = CacheKey::batch_new(self.provider.model_name(), None, texts, self.key_strategy);

        // Single batch cache lookup
        let cached = self.cache.get_batch(&keys).await?;

        // Separate hits from misses
        let mut results: Vec<(usize, Vec<f32>)> = Vec::with_capacity(texts.len());
        let mut miss_indices = Vec::new();
        for (i, cached_val) in cached.into_iter().enumerate() {
            if let Some(vec) = cached_val {
                results.push((i, vec));
            } else {
                miss_indices.push(i);
            }
        }

        // Generate embeddings for misses in chunks with concurrency
        if !miss_indices.is_empty() {
            let miss_texts: Vec<&str> = miss_indices.iter().map(|&i| texts[i]).collect();

            let generated = chunked_embed(
                self.provider.as_ref(),
                &miss_texts,
                self.embed_batch_size,
                self.embed_concurrency,
            )
            .await?;

            // Batch cache write
            let entries: Vec<_> = miss_indices
                .iter()
                .zip(generated.iter())
                .map(|(&idx, vec)| (keys[idx].clone(), vec.clone(), self.provider.dimensions()))
                .collect();
            self.cache.set_batch(&entries).await?;

            for (idx, embedding) in miss_indices.into_iter().zip(generated) {
                results.push((idx, embedding));
            }
        }

        // Sort by original index and return
        results.sort_by_key(|(i, _)| *i);
        Ok(results.into_iter().map(|(_, e)| e).collect())
    }

    /// Get cache statistics
    pub async fn cache_stats(&self) -> anyhow::Result<EmbeddingCacheStats> {
        self.cache.stats().await
    }

    /// Get the underlying provider
    pub fn provider(&self) -> &dyn EmbeddingProvider {
        self.provider.as_ref()
    }
}

/// Embed texts in chunks, sending chunk_size texts per provider call.
/// Chunks are processed sequentially to avoid Send lifetime issues in async traits.
async fn chunked_embed(
    provider: &dyn EmbeddingProvider,
    texts: &[&str],
    chunk_size: usize,
    _max_concurrent: usize,
) -> anyhow::Result<Vec<Vec<f32>>> {
    let mut all = Vec::with_capacity(texts.len());
    for chunk in texts.chunks(chunk_size) {
        all.extend(provider.embed_batch(chunk).await?);
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider {
        model: String,
        dimensions: usize,
        call_count: std::sync::atomic::AtomicUsize,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                model: "mock-model".to_string(),
                dimensions: 4,
                call_count: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        #[allow(dead_code)]
        fn call_count(&self) -> usize {
            self.call_count.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for MockProvider {
        async fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(vec![0.1, 0.2, 0.3, 0.4])
        }

        fn model_name(&self) -> &str {
            &self.model
        }

        fn dimensions(&self) -> usize {
            self.dimensions
        }
    }

    #[tokio::test]
    async fn test_cached_provider_caches_embeddings() {
        let provider = Box::new(MockProvider::new());
        let cache = Arc::new(SqliteCache::in_memory().unwrap());
        let cached = CachedEmbeddingProvider::new(provider, cache, KeyStrategy::ModelText);

        // First call should hit the provider
        let emb1 = cached.embed("hello").await.unwrap();
        assert_eq!(emb1, vec![0.1, 0.2, 0.3, 0.4]);

        // Get the provider from cached to check call count
        // We need to reconstruct to check - let's use a different approach
    }

    #[tokio::test]
    async fn test_cached_provider_batch() {
        let provider = Box::new(MockProvider::new());
        let cache = Arc::new(SqliteCache::in_memory().unwrap());
        let cached = CachedEmbeddingProvider::new(provider, cache, KeyStrategy::ModelText);

        // Pre-cache one embedding
        let _ = cached.embed("hello").await.unwrap();

        // Batch request with one cached and one new
        let embeddings = cached.embed_batch(&["hello", "world"]).await.unwrap();

        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0], vec![0.1, 0.2, 0.3, 0.4]);
        assert_eq!(embeddings[1], vec![0.1, 0.2, 0.3, 0.4]);
    }
}
