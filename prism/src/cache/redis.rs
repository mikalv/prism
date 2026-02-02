//! Redis-based embedding cache

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use redis::AsyncCommands;

use super::embedding::{CacheKey, EmbeddingCache, EmbeddingCacheStats};

/// Redis-based embedding cache
pub struct RedisCache {
    client: redis::Client,
    prefix: String,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl RedisCache {
    /// Create a new Redis cache connected to the given URL
    pub fn new(url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)?;
        Ok(Self {
            client,
            prefix: "prism:emb:".to_string(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        })
    }

    /// Create with a custom key prefix
    pub fn with_prefix(url: &str, prefix: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)?;
        Ok(Self {
            client,
            prefix: prefix.to_string(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        })
    }

    fn make_key(&self, key: &CacheKey) -> String {
        format!("{}{}", self.prefix, key.hash)
    }

    async fn get_connection(&self) -> anyhow::Result<redis::aio::MultiplexedConnection> {
        Ok(self.client.get_multiplexed_async_connection().await?)
    }
}

/// Convert f32 vector to bytes (little-endian)
fn f32_vec_to_bytes(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Convert bytes to f32 vector (little-endian)
fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

#[async_trait]
impl EmbeddingCache for RedisCache {
    async fn get(&self, key: &CacheKey) -> anyhow::Result<Option<Vec<f32>>> {
        let mut conn = self.get_connection().await?;
        let redis_key = self.make_key(key);

        let result: Option<Vec<u8>> = conn.get(&redis_key).await?;

        match result {
            Some(bytes) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                // Update access time
                let now = chrono::Utc::now().timestamp();
                let meta_key = format!("{}:meta", redis_key);
                let _: Result<(), _> = conn
                    .hset::<_, _, _, ()>(&meta_key, "accessed_at", now)
                    .await;
                Ok(Some(bytes_to_f32_vec(&bytes)))
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                Ok(None)
            }
        }
    }

    async fn set(&self, key: &CacheKey, vector: Vec<f32>, dimensions: usize) -> anyhow::Result<()> {
        let mut conn = self.get_connection().await?;
        let redis_key = self.make_key(key);
        let bytes = f32_vec_to_bytes(&vector);
        let now = chrono::Utc::now().timestamp();

        // Store vector data
        conn.set::<_, _, ()>(&redis_key, bytes).await?;

        // Store metadata
        let meta_key = format!("{}:meta", redis_key);
        redis::pipe()
            .hset(&meta_key, "model", &key.model)
            .hset(&meta_key, "dimensions", dimensions)
            .hset(&meta_key, "created_at", now)
            .hset(&meta_key, "accessed_at", now)
            .exec_async(&mut conn)
            .await?;

        // Track key in a set for enumeration
        let index_key = format!("{}__index", self.prefix);
        conn.sadd::<_, _, ()>(&index_key, &redis_key).await?;

        Ok(())
    }

    async fn stats(&self) -> anyhow::Result<EmbeddingCacheStats> {
        let mut conn = self.get_connection().await?;
        let index_key = format!("{}__index", self.prefix);

        let total_entries: usize = conn.scard(&index_key).await.unwrap_or(0);

        Ok(EmbeddingCacheStats {
            total_entries,
            total_bytes: 0, // Not easily tracked in Redis without scanning
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
        })
    }

    async fn evict_lru(&self, max_entries: usize) -> anyhow::Result<usize> {
        let mut conn = self.get_connection().await?;
        let index_key = format!("{}__index", self.prefix);

        let total: usize = conn.scard(&index_key).await.unwrap_or(0);
        if total <= max_entries {
            return Ok(0);
        }

        // Collect all keys with their access times
        let keys: Vec<String> = conn.smembers(&index_key).await?;
        let mut key_times: Vec<(String, i64)> = Vec::new();

        for key in &keys {
            let meta_key = format!("{}:meta", key);
            let accessed_at: i64 = conn.hget(&meta_key, "accessed_at").await.unwrap_or(0);
            key_times.push((key.clone(), accessed_at));
        }

        // Sort by access time (oldest first)
        key_times.sort_by_key(|(_, t)| *t);

        let to_delete = total - max_entries;
        let mut deleted = 0;

        for (key, _) in key_times.into_iter().take(to_delete) {
            let meta_key = format!("{}:meta", key);
            conn.del::<_, ()>(&key).await?;
            conn.del::<_, ()>(&meta_key).await?;
            conn.srem::<_, _, ()>(&index_key, &key).await?;
            deleted += 1;
        }

        Ok(deleted)
    }

    async fn clear(&self) -> anyhow::Result<usize> {
        let mut conn = self.get_connection().await?;
        let index_key = format!("{}__index", self.prefix);

        let keys: Vec<String> = conn.smembers(&index_key).await?;
        let count = keys.len();

        for key in &keys {
            let meta_key = format!("{}:meta", key);
            conn.del::<_, ()>(key).await?;
            conn.del::<_, ()>(&meta_key).await?;
        }
        conn.del::<_, ()>(&index_key).await?;

        Ok(count)
    }

    async fn clear_older_than(&self, timestamp: i64) -> anyhow::Result<usize> {
        let mut conn = self.get_connection().await?;
        let index_key = format!("{}__index", self.prefix);

        let keys: Vec<String> = conn.smembers(&index_key).await?;
        let mut deleted = 0;

        for key in &keys {
            let meta_key = format!("{}:meta", key);
            let accessed_at: i64 = conn.hget(&meta_key, "accessed_at").await.unwrap_or(0);

            if accessed_at < timestamp {
                conn.del::<_, ()>(key).await?;
                conn.del::<_, ()>(&meta_key).await?;
                conn.srem::<_, _, ()>(&index_key, key).await?;
                deleted += 1;
            }
        }

        Ok(deleted)
    }
}
