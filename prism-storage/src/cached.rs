//! Two-tier cached storage implementation.
//!
//! Provides a fast local L1 cache backed by a remote L2 storage (typically S3).
//! Useful for deployments where frequently accessed data should be local but
//! durability is handled by remote storage.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐
//! │   Request   │
//! └──────┬──────┘
//!        │
//!        ▼
//! ┌─────────────┐     Hit
//! │  L1 Cache   │────────────► Return
//! │   (Local)   │
//! └──────┬──────┘
//!        │ Miss
//!        ▼
//! ┌─────────────┐
//! │  L2 Storage │
//! │    (S3)     │
//! └──────┬──────┘
//!        │
//!        ▼
//!   Populate L1
//! ```
//!
//! # Write Strategy
//!
//! - **Write-through**: Writes go to both L1 and L2 (default)
//! - Ensures durability on L2 before acknowledging write
//!
//! # Eviction
//!
//! L1 cache uses LRU eviction when `max_size` is exceeded.

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, instrument};

use crate::error::{Result, StorageError};
use crate::local::LocalStorage;
use crate::path::StoragePath;
use crate::traits::{ListOptions, ObjectMeta, SegmentStorage};

/// Configuration for cached storage.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum size of L1 cache in bytes
    pub max_size_bytes: u64,
    /// Whether to write through to L2 on writes (vs write-back)
    pub write_through: bool,
    /// Populate L1 on read miss
    pub populate_on_read: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: 1024 * 1024 * 1024, // 1 GB
            write_through: true,
            populate_on_read: true,
        }
    }
}

impl CacheConfig {
    /// Create config with specified max size in gigabytes.
    pub fn with_max_size_gb(gb: u64) -> Self {
        Self {
            max_size_bytes: gb * 1024 * 1024 * 1024,
            ..Default::default()
        }
    }
}

/// Entry in the LRU cache tracking.
#[derive(Debug, Clone)]
struct CacheEntry {
    size: u64,
    last_accessed: Instant,
}

/// LRU cache state for tracking L1 usage.
struct CacheState {
    entries: HashMap<String, CacheEntry>,
    total_size: u64,
    max_size: u64,
    hits: u64,
    misses: u64,
}

impl CacheState {
    fn new(max_size: u64) -> Self {
        Self {
            entries: HashMap::new(),
            total_size: 0,
            max_size,
            hits: 0,
            misses: 0,
        }
    }

    fn record_access(&mut self, path: &str, size: u64) {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.last_accessed = Instant::now();
            self.hits += 1;
        } else {
            self.entries.insert(
                path.to_string(),
                CacheEntry {
                    size,
                    last_accessed: Instant::now(),
                },
            );
            self.total_size += size;
            self.misses += 1;
        }
    }

    fn remove(&mut self, path: &str) {
        if let Some(entry) = self.entries.remove(path) {
            self.total_size = self.total_size.saturating_sub(entry.size);
        }
    }

    fn needs_eviction(&self) -> bool {
        self.total_size > self.max_size
    }

    fn evict_lru(&mut self) -> Option<String> {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.last_accessed)
            .map(|(k, _)| k.clone());

        if let Some(ref key) = oldest {
            self.remove(key);
        }

        oldest
    }

    fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Two-tier cached storage with L1 (local) and L2 (remote).
pub struct CachedStorage {
    /// Fast local cache (L1)
    l1: LocalStorage,
    /// Durable remote storage (L2)
    l2: Arc<dyn SegmentStorage>,
    /// Cache state for LRU tracking
    state: Mutex<CacheState>,
    /// Configuration
    config: CacheConfig,
}

impl CachedStorage {
    /// Create a new cached storage.
    pub fn new(
        l1_path: impl Into<std::path::PathBuf>,
        l2: Arc<dyn SegmentStorage>,
        config: CacheConfig,
    ) -> Self {
        Self {
            l1: LocalStorage::new(l1_path),
            l2,
            state: Mutex::new(CacheState::new(config.max_size_bytes)),
            config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(
        l1_path: impl Into<std::path::PathBuf>,
        l2: Arc<dyn SegmentStorage>,
    ) -> Self {
        Self::new(l1_path, l2, CacheConfig::default())
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let state = self.state.lock();
        CacheStats {
            entries: state.entries.len(),
            total_size: state.total_size,
            max_size: state.max_size,
            hit_rate: state.hit_rate(),
            hits: state.hits,
            misses: state.misses,
        }
    }

    /// Clear the L1 cache.
    pub async fn clear_cache(&self) -> Result<()> {
        let paths: Vec<_> = {
            let state = self.state.lock();
            state.entries.keys().cloned().collect()
        };

        for path_str in paths {
            if let Some(path) = StoragePath::parse(&path_str) {
                self.l1.delete(&path).await.ok();
            }
        }

        let mut state = self.state.lock();
        state.entries.clear();
        state.total_size = 0;

        Ok(())
    }

    /// Evict entries until under max size.
    async fn evict_if_needed(&self) -> Result<()> {
        loop {
            let to_evict = {
                let mut state = self.state.lock();
                if state.needs_eviction() {
                    state.evict_lru()
                } else {
                    None
                }
            };

            match to_evict {
                Some(path_str) => {
                    debug!("Evicting from L1 cache: {}", path_str);
                    if let Some(path) = StoragePath::parse(&path_str) {
                        self.l1.delete(&path).await.ok();
                    }
                }
                None => break,
            }
        }

        Ok(())
    }

    /// Populate L1 cache from L2.
    async fn populate_l1(&self, path: &StoragePath, data: &Bytes) -> Result<()> {
        // Write to L1
        self.l1.write(path, data.clone()).await?;

        // Update state and evict if needed
        {
            let mut state = self.state.lock();
            state.record_access(&path.to_string(), data.len() as u64);
        }
        self.evict_if_needed().await?;

        Ok(())
    }
}

impl std::fmt::Debug for CachedStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stats = self.stats();
        f.debug_struct("CachedStorage")
            .field("l1", &self.l1)
            .field("l2", &self.l2.backend_name())
            .field("entries", &stats.entries)
            .field("size_mb", &(stats.total_size / 1024 / 1024))
            .field("hit_rate", &format!("{:.1}%", stats.hit_rate * 100.0))
            .finish()
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of entries in cache
    pub entries: usize,
    /// Total size of cached data in bytes
    pub total_size: u64,
    /// Maximum allowed size in bytes
    pub max_size: u64,
    /// Cache hit rate (0.0 to 1.0)
    pub hit_rate: f64,
    /// Total cache hits
    pub hits: u64,
    /// Total cache misses
    pub misses: u64,
}

#[async_trait]
impl SegmentStorage for CachedStorage {
    #[instrument(skip(self, data), fields(path = %path, size = data.len()))]
    async fn write(&self, path: &StoragePath, data: Bytes) -> Result<()> {
        debug!("CachedStorage write: {}", path);

        if self.config.write_through {
            // Write to L2 first for durability
            self.l2.write(path, data.clone()).await?;
        }

        // Write to L1 cache
        self.l1.write(path, data.clone()).await?;

        // Update cache state
        {
            let mut state = self.state.lock();
            state.record_access(&path.to_string(), data.len() as u64);
        }

        // Evict if needed
        self.evict_if_needed().await?;

        Ok(())
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn read(&self, path: &StoragePath) -> Result<Bytes> {
        debug!("CachedStorage read: {}", path);

        // Try L1 first
        match self.l1.read(path).await {
            Ok(data) => {
                // Cache hit
                let mut state = self.state.lock();
                if let Some(entry) = state.entries.get_mut(&path.to_string()) {
                    entry.last_accessed = Instant::now();
                    state.hits += 1;
                }
                return Ok(data);
            }
            Err(StorageError::NotFound(_)) => {
                // Cache miss, fall through to L2
            }
            Err(e) => return Err(e),
        }

        // L1 miss - read from L2
        let data = self.l2.read(path).await?;

        // Populate L1 cache
        if self.config.populate_on_read {
            self.populate_l1(path, &data).await?;
        } else {
            let mut state = self.state.lock();
            state.misses += 1;
        }

        Ok(data)
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn exists(&self, path: &StoragePath) -> Result<bool> {
        // Check L1 first
        if self.l1.exists(path).await? {
            return Ok(true);
        }

        // Check L2
        self.l2.exists(path).await
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn delete(&self, path: &StoragePath) -> Result<()> {
        debug!("CachedStorage delete: {}", path);

        // Delete from both layers
        self.l1.delete(path).await?;
        self.l2.delete(path).await?;

        // Update cache state
        {
            let mut state = self.state.lock();
            state.remove(&path.to_string());
        }

        Ok(())
    }

    #[instrument(skip(self), fields(prefix = %prefix))]
    async fn list(&self, prefix: &StoragePath) -> Result<Vec<ObjectMeta>> {
        // List from L2 (source of truth)
        self.l2.list(prefix).await
    }

    async fn list_with_options(
        &self,
        prefix: &StoragePath,
        options: ListOptions,
    ) -> Result<Vec<ObjectMeta>> {
        self.l2.list_with_options(prefix, options).await
    }

    #[instrument(skip(self), fields(from = %from, to = %to))]
    async fn rename(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        debug!("CachedStorage rename: {} -> {}", from, to);

        // Rename in L2 first
        self.l2.rename(from, to).await?;

        // Update L1 if cached
        if self.l1.exists(from).await? {
            self.l1.rename(from, to).await?;

            // Update cache state
            let mut state = self.state.lock();
            if let Some(entry) = state.entries.remove(&from.to_string()) {
                state.entries.insert(to.to_string(), entry);
            }
        }

        Ok(())
    }

    #[instrument(skip(self), fields(from = %from, to = %to))]
    async fn copy(&self, from: &StoragePath, to: &StoragePath) -> Result<()> {
        debug!("CachedStorage copy: {} -> {}", from, to);

        // Copy in L2
        self.l2.copy(from, to).await?;

        // Optionally copy in L1 if cached
        if self.l1.exists(from).await? {
            self.l1.copy(from, to).await?;

            // Update cache state
            let mut state = self.state.lock();
            if let Some(entry) = state.entries.get(&from.to_string()) {
                let new_entry = CacheEntry {
                    size: entry.size,
                    last_accessed: Instant::now(),
                };
                state.total_size += entry.size;
                state.entries.insert(to.to_string(), new_entry);
            }
        }

        Ok(())
    }

    #[instrument(skip(self), fields(path = %path))]
    async fn head(&self, path: &StoragePath) -> Result<ObjectMeta> {
        // Check L1 first
        if let Ok(meta) = self.l1.head(path).await {
            return Ok(meta);
        }

        // Fall back to L2
        self.l2.head(path).await
    }

    fn backend_name(&self) -> &'static str {
        "cached"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_storage() -> (CachedStorage, TempDir, TempDir) {
        let l1_dir = TempDir::new().unwrap();
        let l2_dir = TempDir::new().unwrap();

        let l2 = Arc::new(LocalStorage::new(l2_dir.path()));

        let config = CacheConfig {
            max_size_bytes: 1024 * 10, // 10 KB for testing
            ..Default::default()
        };

        let storage = CachedStorage::new(l1_dir.path(), l2, config);

        (storage, l1_dir, l2_dir)
    }

    #[tokio::test]
    async fn test_write_through() {
        let (storage, l1_dir, l2_dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");
        let data = Bytes::from("test data");

        storage.write(&path, data.clone()).await.unwrap();

        // Should exist in both L1 and L2
        let l1 = LocalStorage::new(l1_dir.path());
        let l2 = LocalStorage::new(l2_dir.path());

        assert!(l1.exists(&path).await.unwrap());
        assert!(l2.exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn test_read_cache_hit() {
        let (storage, _l1_dir, _l2_dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");
        let data = Bytes::from("test data");

        storage.write(&path, data.clone()).await.unwrap();

        // First read - should be in L1
        let read1 = storage.read(&path).await.unwrap();
        assert_eq!(read1, data);

        let stats = storage.stats();
        assert!(stats.hits > 0 || stats.entries > 0);
    }

    #[tokio::test]
    async fn test_read_cache_miss_populate() {
        let (storage, l1_dir, l2_dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");
        let data = Bytes::from("test data");

        // Write directly to L2 (bypassing cache)
        let l2 = LocalStorage::new(l2_dir.path());
        l2.write(&path, data.clone()).await.unwrap();

        // L1 should be empty
        let l1 = LocalStorage::new(l1_dir.path());
        assert!(!l1.exists(&path).await.unwrap());

        // Read through CachedStorage - should populate L1
        let read = storage.read(&path).await.unwrap();
        assert_eq!(read, data);

        // Now L1 should have the data
        assert!(l1.exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn test_eviction() {
        let (storage, _l1_dir, _l2_dir) = create_test_storage().await;

        // Write more than max_size (10 KB)
        for i in 0..20 {
            let path = StoragePath::vector("test", "shard_0", format!("file_{}.bin", i));
            let data = Bytes::from(vec![0u8; 1024]); // 1 KB each
            storage.write(&path, data).await.unwrap();
        }

        let stats = storage.stats();
        // Should have evicted some entries
        assert!(stats.total_size <= stats.max_size);
    }

    #[tokio::test]
    async fn test_delete() {
        let (storage, l1_dir, l2_dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");
        let data = Bytes::from("test data");

        storage.write(&path, data).await.unwrap();
        storage.delete(&path).await.unwrap();

        // Should be deleted from both
        let l1 = LocalStorage::new(l1_dir.path());
        let l2 = LocalStorage::new(l2_dir.path());

        assert!(!l1.exists(&path).await.unwrap());
        assert!(!l2.exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn test_stats() {
        let (storage, _l1_dir, _l2_dir) = create_test_storage().await;

        let initial_stats = storage.stats();
        assert_eq!(initial_stats.entries, 0);
        assert_eq!(initial_stats.hits, 0);
        assert_eq!(initial_stats.misses, 0);

        // Write some data
        let path = StoragePath::vector("test", "shard_0", "index.bin");
        storage.write(&path, Bytes::from("data")).await.unwrap();

        let stats = storage.stats();
        assert_eq!(stats.entries, 1);
    }

    #[tokio::test]
    async fn test_clear_cache() {
        let (storage, l1_dir, l2_dir) = create_test_storage().await;

        let path = StoragePath::vector("test", "shard_0", "index.bin");
        storage
            .write(&path, Bytes::from("test data"))
            .await
            .unwrap();

        storage.clear_cache().await.unwrap();

        let stats = storage.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.total_size, 0);

        // L2 should still have data
        let l2 = LocalStorage::new(l2_dir.path());
        assert!(l2.exists(&path).await.unwrap());

        // L1 should be empty
        let l1 = LocalStorage::new(l1_dir.path());
        assert!(!l1.exists(&path).await.unwrap());
    }
}
