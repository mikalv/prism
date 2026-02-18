//! Cache trait and shared types

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use super::KeyStrategy;

/// Cache key for embedding lookups
#[derive(Debug, Clone)]
pub struct CacheKey {
    /// The hash used for lookup
    pub hash: String,
    /// Model name (for debugging/stats)
    pub model: String,
    /// Model version (optional)
    pub model_version: Option<String>,
    /// Original text hash (for debugging)
    pub text_hash: String,
}

impl CacheKey {
    /// Create a cache key based on the strategy
    pub fn new(
        model: &str,
        model_version: Option<&str>,
        text: &str,
        strategy: KeyStrategy,
    ) -> Self {
        let text_hash = hex_hash(text);

        let hash = match strategy {
            KeyStrategy::TextOnly => text_hash.clone(),
            KeyStrategy::ModelText => hex_hash(&format!("{}:{}", model, text)),
            KeyStrategy::ModelVersionText => {
                let version = model_version.unwrap_or("unknown");
                hex_hash(&format!("{}:{}:{}", model, version, text))
            }
        };

        Self {
            hash,
            model: model.to_string(),
            model_version: model_version.map(String::from),
            text_hash,
        }
    }

    /// Create cache keys for a batch of texts
    pub fn batch_new(
        model: &str,
        model_version: Option<&str>,
        texts: &[&str],
        strategy: KeyStrategy,
    ) -> Vec<Self> {
        texts
            .iter()
            .map(|t| Self::new(model, model_version, t, strategy))
            .collect()
    }
}

fn hex_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// Statistics about the embedding cache
#[derive(Debug, Clone, Default)]
pub struct EmbeddingCacheStats {
    /// Total number of cached embeddings
    pub total_entries: usize,
    /// Total size in bytes
    pub total_bytes: usize,
    /// Cache hit count (since startup)
    pub hits: u64,
    /// Cache miss count (since startup)
    pub misses: u64,
}

impl EmbeddingCacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Trait for embedding cache backends
#[async_trait]
pub trait EmbeddingCache: Send + Sync {
    /// Get an embedding from the cache
    async fn get(&self, key: &CacheKey) -> anyhow::Result<Option<Vec<f32>>>;

    /// Store an embedding in the cache
    async fn set(&self, key: &CacheKey, vector: Vec<f32>, dimensions: usize) -> anyhow::Result<()>;

    /// Get cache statistics
    async fn stats(&self) -> anyhow::Result<EmbeddingCacheStats>;

    /// Evict least recently used entries to stay under max_entries
    async fn evict_lru(&self, max_entries: usize) -> anyhow::Result<usize>;

    /// Clear all entries
    async fn clear(&self) -> anyhow::Result<usize>;

    /// Clear entries older than the given timestamp
    async fn clear_older_than(&self, timestamp: i64) -> anyhow::Result<usize>;

    /// Get embeddings for multiple keys in a single batch operation.
    /// Default implementation falls back to sequential get() calls.
    async fn get_batch(&self, keys: &[CacheKey]) -> anyhow::Result<Vec<Option<Vec<f32>>>> {
        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            results.push(self.get(key).await?);
        }
        Ok(results)
    }

    /// Store multiple embeddings in a single batch operation.
    /// Default implementation falls back to sequential set() calls.
    async fn set_batch(&self, entries: &[(CacheKey, Vec<f32>, usize)]) -> anyhow::Result<()> {
        for (key, vector, dims) in entries {
            self.set(key, vector.clone(), *dims).await?;
        }
        Ok(())
    }
}

// Need hex crate for encoding
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}
