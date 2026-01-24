//! Embedding cache module for Prism
//!
//! Provides caching layer for embeddings to avoid repeated calls to embedding providers.
//! Supports SQLite (default) and Redis backends.

mod sqlite;
mod r#trait;

pub use sqlite::SqliteCache;
pub use r#trait::{CacheKey, CacheStats, EmbeddingCache};

#[cfg(feature = "cache-redis")]
mod redis;

#[cfg(feature = "cache-redis")]
pub use redis::RedisCache;

use serde::{Deserialize, Serialize};

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Backend type: "sqlite" or "redis"
    pub backend: String,

    /// Path for SQLite database
    pub path: Option<String>,

    /// URL for Redis connection
    pub url: Option<String>,

    /// Maximum number of cached entries (for LRU eviction)
    pub max_entries: Option<usize>,

    /// Cache key strategy
    #[serde(default = "default_key_strategy")]
    pub key_strategy: KeyStrategy,
}

fn default_key_strategy() -> KeyStrategy {
    KeyStrategy::ModelText
}

/// Strategy for generating cache keys
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeyStrategy {
    /// Hash only the text (ignores model)
    TextOnly,

    /// Hash model name + text (default)
    #[default]
    ModelText,

    /// Hash model name + version + text
    ModelVersionText,
}

impl CacheConfig {
    pub fn sqlite(path: &str) -> Self {
        Self {
            backend: "sqlite".to_string(),
            path: Some(path.to_string()),
            url: None,
            max_entries: Some(1_000_000),
            key_strategy: KeyStrategy::ModelText,
        }
    }

    #[cfg(feature = "cache-redis")]
    pub fn redis(url: &str) -> Self {
        Self {
            backend: "redis".to_string(),
            path: None,
            url: Some(url.to_string()),
            max_entries: None,
            key_strategy: KeyStrategy::ModelText,
        }
    }
}
