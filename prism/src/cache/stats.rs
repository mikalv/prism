//! Object cache statistics for object storage operations

use std::sync::atomic::{AtomicU64, Ordering};

/// Cache statistics tracking hits, misses, and evictions for object storage
#[derive(Debug, Default)]
pub struct ObjectCacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
}

impl ObjectCacheStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn evict(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    pub fn evictions(&self) -> u64 {
        self.evictions.load(Ordering::Relaxed)
    }

    pub fn total_requests(&self) -> u64 {
        self.hits() + self.misses()
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            0.0
        } else {
            self.hits() as f64 / total as f64
        }
    }
}
