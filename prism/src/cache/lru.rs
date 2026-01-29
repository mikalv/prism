//! LRU cache for object storage files

use crate::cache::stats::ObjectCacheStats;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// LRU cache entry with access tracking
#[derive(Clone, Debug)]
pub(crate) struct LruEntry {
    key: PathBuf,
    size: u64,
    last_accessed: Arc<AtomicU64>,
}

/// LRU cache for object storage files
pub struct LruCache {
    entries: Arc<RwLock<HashMap<PathBuf, Arc<LruEntry>>>>,
    max_size_bytes: u64,
    current_size_bytes: Arc<AtomicU64>,
    access_counter: Arc<AtomicU64>,
    stats: Arc<ObjectCacheStats>,
}

impl LruCache {
    pub fn new(max_size_mb: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_size_bytes: (max_size_mb as u64) * 1024 * 1024,
            current_size_bytes: Arc::new(AtomicU64::new(0)),
            access_counter: Arc::new(AtomicU64::new(0)),
            stats: Arc::new(ObjectCacheStats::new()),
        }
    }

    pub fn stats(&self) -> Arc<ObjectCacheStats> {
        Arc::clone(&self.stats)
    }

    pub(crate) fn get(&self, key: &PathBuf) -> Option<Arc<LruEntry>> {
        let entries = self.entries.read().ok()?;

        if let Some(entry) = entries.get(key) {
            self.stats.hit();
            let access = self.access_counter.fetch_add(1, Ordering::Relaxed);
            entry.last_accessed.store(access, Ordering::Relaxed);
            Some(Arc::clone(entry))
        } else {
            self.stats.miss();
            None
        }
    }

    pub(crate) fn put(&self, key: PathBuf, size: u64) -> Option<PathBuf> {
        let mut entries = match self.entries.write() {
            Ok(guard) => guard,
            Err(_) => return None,
        };

        // Check if already exists
        if entries.contains_key(&key) {
            return None;
        }

        // Check if we need to evict
        let current_size = self.current_size_bytes.load(Ordering::Relaxed);
        let mut evicted = None;
        if current_size + size > self.max_size_bytes {
            evicted = Self::evict_lru(&mut entries, &self.current_size_bytes, &self.stats);
        }

        // Double-check after eviction
        let current_size = self.current_size_bytes.load(Ordering::Relaxed);
        if current_size + size > self.max_size_bytes {
            return evicted;
        }

        // Insert new entry
        let access = self.access_counter.fetch_add(1, Ordering::Relaxed);
        let entry = Arc::new(LruEntry {
            key: key.clone(),
            size,
            last_accessed: Arc::new(AtomicU64::new(access)),
        });

        entries.insert(key, entry);
        self.current_size_bytes.fetch_add(size, Ordering::Relaxed);
        evicted
    }

    pub fn remove(&self, key: &PathBuf) -> Option<u64> {
        let mut entries = self.entries.write().ok()?;
        let entry = entries.remove(key)?;
        self.current_size_bytes
            .fetch_sub(entry.size, Ordering::Relaxed);
        Some(entry.size)
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
            self.current_size_bytes.store(0, Ordering::Relaxed);
        }
    }

    pub fn size_bytes(&self) -> u64 {
        self.current_size_bytes.load(Ordering::Relaxed)
    }

    pub fn entry_count(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    pub fn size_mb(&self) -> f64 {
        self.size_bytes() as f64 / (1024.0 * 1024.0)
    }

    pub fn max_size_mb(&self) -> f64 {
        self.max_size_bytes as f64 / (1024.0 * 1024.0)
    }

    pub fn hit_rate(&self) -> f64 {
        self.stats.hit_rate()
    }

    fn evict_lru(
        entries: &mut HashMap<PathBuf, Arc<LruEntry>>,
        current_size: &AtomicU64,
        stats: &ObjectCacheStats,
    ) -> Option<PathBuf> {
        if entries.is_empty() {
            return None;
        }

        // Find LRU entry
        let lru_key = entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed.load(Ordering::Relaxed))
            .map(|(k, _)| k.clone());

        if let Some(key) = lru_key {
            if let Some(entry) = entries.remove(&key) {
                current_size.fetch_sub(entry.size, Ordering::Relaxed);
                stats.evict();
                return Some(key);
            }
        }
        None
    }
}
