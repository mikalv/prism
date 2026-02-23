//! LRU cache for object storage files

use crate::cache::stats::ObjectCacheStats;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// LRU cache entry with access tracking
#[derive(Clone, Debug)]
pub(crate) struct LruEntry {
    #[allow(dead_code)]
    key: PathBuf,
    size: u64,
    last_accessed: Arc<AtomicU64>,
}

/// LRU cache for object storage files
#[derive(Clone)]
pub struct LruCache {
    entries: Arc<RwLock<HashMap<PathBuf, Arc<LruEntry>>>>,
    max_size_bytes: u64,
    current_size_bytes: Arc<AtomicU64>,
    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub(crate) fn get(&self, key: &PathBuf) -> Option<Arc<LruEntry>> {
        let entries = self.entries.read();

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

    #[allow(dead_code)]
    pub(crate) fn put(&self, key: PathBuf, size: u64) -> Option<PathBuf> {
        let mut entries = self.entries.write();

        // Check if already exists
        if entries.contains_key(&key) {
            return None;
        }

        // Evict entries until we have enough space
        let mut last_evicted = None;
        while self.current_size_bytes.load(Ordering::Relaxed) + size > self.max_size_bytes {
            match Self::evict_lru(&mut entries, &self.current_size_bytes, &self.stats) {
                Some(evicted_key) => last_evicted = Some(evicted_key),
                None => break, // Nothing left to evict
            }
        }

        // Check if we have enough space after eviction
        let current_size = self.current_size_bytes.load(Ordering::Relaxed);
        if current_size + size > self.max_size_bytes {
            return last_evicted;
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
        last_evicted
    }

    pub fn remove(&self, key: &PathBuf) -> Option<u64> {
        let mut entries = self.entries.write();
        let entry = entries.remove(key)?;
        self.current_size_bytes
            .fetch_sub(entry.size, Ordering::Relaxed);
        Some(entry.size)
    }

    pub fn clear(&self) {
        let mut entries = self.entries.write();
        entries.clear();
        self.current_size_bytes.store(0, Ordering::Relaxed);
    }

    pub fn size_bytes(&self) -> u64 {
        self.current_size_bytes.load(Ordering::Relaxed)
    }

    pub fn entry_count(&self) -> usize {
        self.entries.read().len()
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

    #[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_cache_is_empty() {
        let cache = LruCache::new(10);
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.size_bytes(), 0);
        assert_eq!(cache.size_mb(), 0.0);
        assert_eq!(cache.max_size_mb(), 10.0);
    }

    #[test]
    fn test_put_and_get() {
        let cache = LruCache::new(10);
        let key = PathBuf::from("/tmp/test.bin");
        cache.put(key.clone(), 1024);
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.size_bytes(), 1024);

        let entry = cache.get(&key);
        assert!(entry.is_some());
    }

    #[test]
    fn test_get_missing_key() {
        let cache = LruCache::new(10);
        let key = PathBuf::from("/tmp/missing.bin");
        let entry = cache.get(&key);
        assert!(entry.is_none());
    }

    #[test]
    fn test_put_duplicate_returns_none() {
        let cache = LruCache::new(10);
        let key = PathBuf::from("/tmp/test.bin");
        cache.put(key.clone(), 1024);
        let result = cache.put(key, 2048);
        assert!(result.is_none());
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.size_bytes(), 1024);
    }

    #[test]
    fn test_eviction_on_capacity() {
        // 1 MB cache
        let cache = LruCache::new(1);
        let max = 1024 * 1024;

        // Fill the cache
        let key1 = PathBuf::from("/tmp/a.bin");
        cache.put(key1.clone(), max / 2);

        let key2 = PathBuf::from("/tmp/b.bin");
        cache.put(key2.clone(), max / 2);

        // This should trigger eviction of key1 (LRU)
        let key3 = PathBuf::from("/tmp/c.bin");
        let evicted = cache.put(key3, max / 2);
        assert_eq!(evicted, Some(key1));
    }

    #[test]
    fn test_lru_ordering() {
        // 1 MB cache
        let cache = LruCache::new(1);
        let chunk = 300 * 1024; // 300 KB each, 3 fit in 1 MB

        let key1 = PathBuf::from("/tmp/1.bin");
        let key2 = PathBuf::from("/tmp/2.bin");
        let key3 = PathBuf::from("/tmp/3.bin");

        cache.put(key1.clone(), chunk);
        cache.put(key2.clone(), chunk);
        cache.put(key3.clone(), chunk);

        // Access key1 to make it recently used
        cache.get(&key1);

        // Insert key4, should evict key2 (least recently used)
        let key4 = PathBuf::from("/tmp/4.bin");
        let evicted = cache.put(key4, chunk);
        assert_eq!(evicted, Some(key2));
    }

    #[test]
    fn test_cache_stats_hits_misses() {
        let cache = LruCache::new(10);
        let key = PathBuf::from("/tmp/test.bin");
        cache.put(key.clone(), 1024);

        cache.get(&key); // hit
        cache.get(&key); // hit
        cache.get(&PathBuf::from("/tmp/miss.bin")); // miss

        let stats = cache.stats();
        assert_eq!(stats.hits(), 2);
        assert_eq!(stats.misses(), 1);
        assert_eq!(stats.total_requests(), 3);
    }

    #[test]
    fn test_hit_rate() {
        let cache = LruCache::new(10);
        // No requests → 0.0
        assert_eq!(cache.hit_rate(), 0.0);

        let key = PathBuf::from("/tmp/test.bin");
        cache.put(key.clone(), 1024);
        cache.get(&key); // hit
        cache.get(&PathBuf::from("/missing")); // miss

        // 1 hit / 2 requests = 0.5
        assert!((cache.hit_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_remove() {
        let cache = LruCache::new(10);
        let key = PathBuf::from("/tmp/test.bin");
        cache.put(key.clone(), 2048);
        assert_eq!(cache.entry_count(), 1);

        let removed_size = cache.remove(&key);
        assert_eq!(removed_size, Some(2048));
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.size_bytes(), 0);
    }

    #[test]
    fn test_remove_missing() {
        let cache = LruCache::new(10);
        let key = PathBuf::from("/tmp/missing.bin");
        let removed = cache.remove(&key);
        assert!(removed.is_none());
    }

    #[test]
    fn test_clear() {
        let cache = LruCache::new(10);
        cache.put(PathBuf::from("/tmp/a.bin"), 1024);
        cache.put(PathBuf::from("/tmp/b.bin"), 2048);
        assert_eq!(cache.entry_count(), 2);

        cache.clear();
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.size_bytes(), 0);
    }

    #[test]
    fn test_eviction_stats() {
        let cache = LruCache::new(1);
        let max = 1024 * 1024;

        cache.put(PathBuf::from("/tmp/a.bin"), max);
        // This triggers eviction
        cache.put(PathBuf::from("/tmp/b.bin"), max);

        let stats = cache.stats();
        assert_eq!(stats.evictions(), 1);
    }
}
