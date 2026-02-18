//! SQLite-based embedding cache

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use super::embedding::{CacheKey, EmbeddingCache, EmbeddingCacheStats};

/// SQLite-based embedding cache
pub struct SqliteCache {
    conn: Mutex<Connection>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl SqliteCache {
    /// Create a new SQLite cache at the given path
    pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Create table if not exists
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS embeddings (
                key_hash     TEXT PRIMARY KEY,
                model        TEXT NOT NULL,
                text_hash    TEXT NOT NULL,
                vector       BLOB NOT NULL,
                dimensions   INTEGER NOT NULL,
                created_at   INTEGER NOT NULL,
                accessed_at  INTEGER NOT NULL,
                access_count INTEGER DEFAULT 1
            )
            "#,
            [],
        )?;

        // Create index for LRU eviction
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_accessed ON embeddings(accessed_at)",
            [],
        )?;

        // WAL mode + performance pragmas
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-64000;
             PRAGMA temp_store=MEMORY;",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        })
    }

    /// Create an in-memory cache (for testing)
    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;

        conn.execute(
            r#"
            CREATE TABLE embeddings (
                key_hash     TEXT PRIMARY KEY,
                model        TEXT NOT NULL,
                text_hash    TEXT NOT NULL,
                vector       BLOB NOT NULL,
                dimensions   INTEGER NOT NULL,
                created_at   INTEGER NOT NULL,
                accessed_at  INTEGER NOT NULL,
                access_count INTEGER DEFAULT 1
            )
            "#,
            [],
        )?;

        conn.execute("CREATE INDEX idx_accessed ON embeddings(accessed_at)", [])?;

        // Performance pragmas (WAL not needed for in-memory but cache_size helps)
        conn.execute_batch(
            "PRAGMA cache_size=-64000;
             PRAGMA temp_store=MEMORY;",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        })
    }
}

#[async_trait]
impl EmbeddingCache for SqliteCache {
    async fn get(&self, key: &CacheKey) -> anyhow::Result<Option<Vec<f32>>> {
        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().timestamp();

        let result: Result<Vec<u8>, _> = conn.query_row(
            "SELECT vector FROM embeddings WHERE key_hash = ?",
            params![&key.hash],
            |row| row.get(0),
        );

        match result {
            Ok(bytes) => {
                // Update access time and count
                conn.execute(
                    "UPDATE embeddings SET accessed_at = ?, access_count = access_count + 1 WHERE key_hash = ?",
                    params![now, &key.hash],
                )?;

                self.hits.fetch_add(1, Ordering::Relaxed);
                metrics::counter!("prism_embedding_cache_hits_total", "layer" => "sqlite")
                    .increment(1);

                // Deserialize f32 vector from bytes
                let vector = bytes_to_f32_vec(&bytes);
                Ok(Some(vector))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                metrics::counter!("prism_embedding_cache_misses_total", "layer" => "sqlite")
                    .increment(1);
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn set(&self, key: &CacheKey, vector: Vec<f32>, dimensions: usize) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().timestamp();
        let bytes = f32_vec_to_bytes(&vector);

        conn.execute(
            r#"
            INSERT OR REPLACE INTO embeddings
            (key_hash, model, text_hash, vector, dimensions, created_at, accessed_at, access_count)
            VALUES (?, ?, ?, ?, ?, ?, ?, 1)
            "#,
            params![
                &key.hash,
                &key.model,
                &key.text_hash,
                &bytes,
                dimensions as i64,
                now,
                now
            ],
        )?;

        Ok(())
    }

    async fn stats(&self) -> anyhow::Result<EmbeddingCacheStats> {
        let conn = self.conn.lock().await;

        let total_entries: usize =
            conn.query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))?;

        let total_bytes: usize = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(vector)), 0) FROM embeddings",
            [],
            |row| row.get(0),
        )?;

        Ok(EmbeddingCacheStats {
            total_entries,
            total_bytes,
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
        })
    }

    async fn evict_lru(&self, max_entries: usize) -> anyhow::Result<usize> {
        let conn = self.conn.lock().await;

        let current_count: usize =
            conn.query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))?;

        if current_count <= max_entries {
            return Ok(0);
        }

        let to_delete = current_count - max_entries;

        conn.execute(
            r#"
            DELETE FROM embeddings WHERE key_hash IN (
                SELECT key_hash FROM embeddings
                ORDER BY accessed_at ASC
                LIMIT ?
            )
            "#,
            params![to_delete as i64],
        )?;

        Ok(to_delete)
    }

    async fn clear(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().await;
        let count: usize =
            conn.query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))?;
        conn.execute("DELETE FROM embeddings", [])?;
        Ok(count)
    }

    async fn clear_older_than(&self, timestamp: i64) -> anyhow::Result<usize> {
        let conn = self.conn.lock().await;
        let deleted = conn.execute(
            "DELETE FROM embeddings WHERE accessed_at < ?",
            params![timestamp],
        )?;
        Ok(deleted)
    }

    async fn get_batch(&self, keys: &[CacheKey]) -> anyhow::Result<Vec<Option<Vec<f32>>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().await;

        // Build SELECT ... WHERE key_hash IN (?, ?, ...)
        let placeholders: Vec<&str> = keys.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT key_hash, vector FROM embeddings WHERE key_hash IN ({})",
            placeholders.join(",")
        );

        let mut stmt = conn.prepare(&sql)?;

        // Bind all key hashes as params
        let params: Vec<&dyn rusqlite::types::ToSql> = keys
            .iter()
            .map(|k| &k.hash as &dyn rusqlite::types::ToSql)
            .collect();

        let mut found: std::collections::HashMap<String, Vec<f32>> = std::collections::HashMap::new();
        let mut rows = stmt.query(params.as_slice())?;
        while let Some(row) = rows.next()? {
            let hash: String = row.get(0)?;
            let bytes: Vec<u8> = row.get(1)?;
            found.insert(hash, bytes_to_f32_vec(&bytes));
        }

        // Build results in original order, track hits/misses
        let mut hits: u64 = 0;
        let mut misses: u64 = 0;
        let results = keys
            .iter()
            .map(|k| {
                if let Some(vec) = found.remove(&k.hash) {
                    hits += 1;
                    Some(vec)
                } else {
                    misses += 1;
                    None
                }
            })
            .collect();

        // Skip per-row access-time update for batch reads (perf optimization)
        self.hits.fetch_add(hits, Ordering::Relaxed);
        self.misses.fetch_add(misses, Ordering::Relaxed);
        if hits > 0 {
            metrics::counter!("prism_embedding_cache_hits_total", "layer" => "sqlite")
                .increment(hits);
        }
        if misses > 0 {
            metrics::counter!("prism_embedding_cache_misses_total", "layer" => "sqlite")
                .increment(misses);
        }

        Ok(results)
    }

    async fn set_batch(&self, entries: &[(CacheKey, Vec<f32>, usize)]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().timestamp();

        conn.execute_batch("BEGIN")?;
        {
            let mut stmt = conn.prepare(
                r#"INSERT OR REPLACE INTO embeddings
                   (key_hash, model, text_hash, vector, dimensions, created_at, accessed_at, access_count)
                   VALUES (?, ?, ?, ?, ?, ?, ?, 1)"#,
            )?;
            for (key, vector, dims) in entries {
                let bytes = f32_vec_to_bytes(vector);
                stmt.execute(params![
                    &key.hash,
                    &key.model,
                    &key.text_hash,
                    &bytes,
                    *dims as i64,
                    now,
                    now
                ])?;
            }
        }
        conn.execute_batch("COMMIT")?;

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_roundtrip() {
        let cache = SqliteCache::in_memory().unwrap();

        let key = CacheKey::new(
            "test-model",
            None,
            "hello world",
            super::super::KeyStrategy::ModelText,
        );
        let vector = vec![0.1, 0.2, 0.3, 0.4];

        // Initially empty
        assert!(cache.get(&key).await.unwrap().is_none());

        // Set and get
        cache.set(&key, vector.clone(), 4).await.unwrap();
        let retrieved = cache.get(&key).await.unwrap().unwrap();

        assert_eq!(retrieved.len(), vector.len());
        for (a, b) in retrieved.iter().zip(vector.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = SqliteCache::in_memory().unwrap();

        let key1 = CacheKey::new("model", None, "text1", super::super::KeyStrategy::ModelText);
        let key2 = CacheKey::new("model", None, "text2", super::super::KeyStrategy::ModelText);

        cache.set(&key1, vec![0.1, 0.2], 2).await.unwrap();
        cache.set(&key2, vec![0.3, 0.4], 2).await.unwrap();

        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.total_bytes, 16); // 2 vectors * 2 floats * 4 bytes
    }

    #[tokio::test]
    async fn test_lru_eviction() {
        let cache = SqliteCache::in_memory().unwrap();

        // Add 5 entries
        for i in 0..5 {
            let key = CacheKey::new(
                "model",
                None,
                &format!("text{}", i),
                super::super::KeyStrategy::ModelText,
            );
            cache.set(&key, vec![0.1], 1).await.unwrap();
        }

        // Evict down to 3
        let evicted = cache.evict_lru(3).await.unwrap();
        assert_eq!(evicted, 2);

        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.total_entries, 3);
    }
}
