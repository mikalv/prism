//! Vector segment: an immutable (once sealed) HNSW index with tombstone support.

use crate::backends::r#trait::SearchResult;
use crate::error::Result;
use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use super::index::{HnswBackend, HnswIndex, Metric};

/// Unique segment identifier (monotonic per shard).
pub type SegmentId = u64;

/// A single HNSW segment with tombstone tracking.
///
/// Segments start as "active" (accepting writes) and can be sealed to become
/// immutable. Sealed segments are eligible for compaction.
pub struct VectorSegment {
    pub id: SegmentId,
    pub hnsw: HnswBackend,
    pub tombstones: RoaringBitmap,
    pub id_to_key: HashMap<String, u32>,
    pub key_to_id: HashMap<u32, String>,
    pub next_key: AtomicU32,
    pub documents: HashMap<String, HashMap<String, serde_json::Value>>,
    pub dimensions: usize,
    pub metric: Metric,
    pub sealed: bool,
}

/// Serializable segment state for persistence.
#[derive(Serialize, Deserialize)]
pub struct PersistedSegment {
    pub id: SegmentId,
    pub dimensions: usize,
    pub metric: Metric,
    pub id_to_key: HashMap<String, u32>,
    pub key_to_id: HashMap<u32, String>,
    pub next_key: u32,
    pub documents: HashMap<String, HashMap<String, serde_json::Value>>,
    pub tombstones: Vec<u32>,
    pub hnsw_data: Vec<u8>,
    pub sealed: bool,
}

impl VectorSegment {
    /// Create a new empty active segment.
    pub fn new(
        id: SegmentId,
        dimensions: usize,
        metric: Metric,
        m: usize,
        ef_construction: usize,
    ) -> Result<Self> {
        let hnsw = HnswBackend::new(dimensions, metric, m, ef_construction)?;
        Ok(Self {
            id,
            hnsw,
            tombstones: RoaringBitmap::new(),
            id_to_key: HashMap::new(),
            key_to_id: HashMap::new(),
            next_key: AtomicU32::new(0),
            documents: HashMap::new(),
            dimensions,
            metric,
            sealed: false,
        })
    }

    /// Add a document with its vector to this segment.
    pub fn add(
        &mut self,
        doc_id: &str,
        vector: &[f32],
        fields: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        if self.sealed {
            return Err(crate::error::Error::Backend(
                "Cannot write to a sealed segment".into(),
            ));
        }
        if vector.len() != self.dimensions {
            return Err(crate::error::Error::Schema(format!(
                "Expected {} dimensions, got {}",
                self.dimensions,
                vector.len()
            )));
        }

        let key = self.next_key.fetch_add(1, Ordering::SeqCst);
        self.hnsw.add(key, vector)?;
        self.id_to_key.insert(doc_id.to_string(), key);
        self.key_to_id.insert(key, doc_id.to_string());
        self.documents.insert(doc_id.to_string(), fields);
        Ok(())
    }

    /// Search this segment, skipping tombstoned keys.
    pub fn search(
        &self,
        query_vector: &[f32],
        k: usize,
        ef_search: usize,
    ) -> Result<Vec<SearchResult>> {
        let matches = self.hnsw.search(query_vector, k, ef_search)?;
        let mut results = Vec::new();
        for (key, score) in matches {
            if self.tombstones.contains(key) {
                continue;
            }
            if let Some(doc_id) = self.key_to_id.get(&key) {
                if let Some(fields) = self.documents.get(doc_id) {
                    results.push(SearchResult {
                        id: doc_id.clone(),
                        score,
                        fields: fields.clone(),
                        highlight: None,
                    });
                }
            }
        }
        Ok(results)
    }

    /// Mark a document as deleted (tombstoned).
    /// Also cleans up the ID mappings and document data.
    pub fn tombstone(&mut self, doc_id: &str) -> bool {
        if let Some(key) = self.id_to_key.remove(doc_id) {
            self.tombstones.insert(key);
            self.key_to_id.remove(&key);
            self.documents.remove(doc_id);
            true
        } else {
            false
        }
    }

    /// Check if this segment contains a document (not tombstoned).
    pub fn contains(&self, doc_id: &str) -> bool {
        self.id_to_key.contains_key(doc_id)
    }

    /// Get a document by ID if it exists and is not tombstoned.
    pub fn get(&self, doc_id: &str) -> Option<&HashMap<String, serde_json::Value>> {
        if self.contains(doc_id) {
            self.documents.get(doc_id)
        } else {
            None
        }
    }

    /// Seal this segment, making it immutable.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Number of live (non-tombstoned) vectors.
    pub fn live_count(&self) -> u64 {
        self.id_to_key.len() as u64
    }

    /// Total number of vectors ever added (including tombstoned).
    pub fn total_count(&self) -> u64 {
        self.id_to_key.len() as u64 + self.tombstones.len()
    }

    /// Number of tombstoned vectors.
    pub fn deleted_count(&self) -> u64 {
        self.tombstones.len()
    }

    /// Check if this segment is a compaction candidate.
    pub fn is_compaction_candidate(&self, delete_ratio_threshold: f32) -> bool {
        if !self.sealed {
            return false;
        }
        let total = self.total_count();
        if total == 0 {
            return false;
        }
        (self.deleted_count() as f32 / total as f32) > delete_ratio_threshold
    }

    /// Serialize this segment for persistence.
    pub fn to_persisted(&self) -> Result<PersistedSegment> {
        let tmp = tempfile::NamedTempFile::new()?;
        self.hnsw.save(tmp.path())?;
        let hnsw_data = std::fs::read(tmp.path())?;

        Ok(PersistedSegment {
            id: self.id,
            dimensions: self.dimensions,
            metric: self.metric,
            id_to_key: self.id_to_key.clone(),
            key_to_id: self.key_to_id.clone(),
            next_key: self.next_key.load(Ordering::SeqCst),
            documents: self.documents.clone(),
            tombstones: self.tombstones.iter().collect(),
            hnsw_data,
            sealed: self.sealed,
        })
    }

    /// Restore a segment from persisted state.
    pub fn from_persisted(p: PersistedSegment) -> Result<Self> {
        let tmp = tempfile::NamedTempFile::new()?;
        std::fs::write(tmp.path(), &p.hnsw_data)?;
        let hnsw = HnswBackend::load(tmp.path())?;

        let mut tombstones = RoaringBitmap::new();
        for k in p.tombstones {
            tombstones.insert(k);
        }

        Ok(Self {
            id: p.id,
            hnsw,
            tombstones,
            id_to_key: p.id_to_key,
            key_to_id: p.key_to_id,
            next_key: AtomicU32::new(p.next_key),
            documents: p.documents,
            dimensions: p.dimensions,
            metric: p.metric,
            sealed: p.sealed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_segment(dim: usize) -> VectorSegment {
        VectorSegment::new(0, dim, Metric::Cosine, 16, 200).unwrap()
    }

    #[test]
    fn test_add_and_search() {
        let mut seg = make_segment(4);
        let fields: HashMap<String, serde_json::Value> = HashMap::new();
        seg.add("doc1", &[1.0, 0.0, 0.0, 0.0], fields.clone())
            .unwrap();
        seg.add("doc2", &[0.0, 1.0, 0.0, 0.0], fields.clone())
            .unwrap();
        seg.add("doc3", &[0.0, 0.0, 1.0, 0.0], fields).unwrap();

        let results = seg.search(&[1.0, 0.0, 0.0, 0.0], 3, 100).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "doc1");
    }

    #[test]
    fn test_tombstone() {
        let mut seg = make_segment(4);
        let fields: HashMap<String, serde_json::Value> = HashMap::new();
        seg.add("doc1", &[1.0, 0.0, 0.0, 0.0], fields.clone())
            .unwrap();
        seg.add("doc2", &[0.0, 1.0, 0.0, 0.0], fields).unwrap();

        assert!(seg.contains("doc1"));
        seg.tombstone("doc1");
        assert!(!seg.contains("doc1"));

        let results = seg.search(&[1.0, 0.0, 0.0, 0.0], 2, 100).unwrap();
        // doc1 should be filtered out
        for r in &results {
            assert_ne!(r.id, "doc1");
        }
    }

    #[test]
    fn test_sealed_segment_rejects_writes() {
        let mut seg = make_segment(4);
        seg.seal();
        let result = seg.add("doc1", &[1.0, 0.0, 0.0, 0.0], HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_compaction_candidate() {
        let mut seg = make_segment(4);
        let fields: HashMap<String, serde_json::Value> = HashMap::new();
        for i in 0..10 {
            seg.add(
                &format!("doc{}", i),
                &[i as f32, 0.0, 0.0, 0.0],
                fields.clone(),
            )
            .unwrap();
        }
        seg.seal();

        // No deletes: not a candidate
        assert!(!seg.is_compaction_candidate(0.2));

        // Delete 3/10 = 0.3 > 0.2 threshold
        seg.tombstone("doc0");
        seg.tombstone("doc1");
        seg.tombstone("doc2");
        assert!(seg.is_compaction_candidate(0.2));
    }

    #[test]
    fn test_persistence_roundtrip() {
        let mut seg = make_segment(4);
        let mut fields = HashMap::new();
        fields.insert(
            "title".to_string(),
            serde_json::Value::String("hello".to_string()),
        );
        seg.add("doc1", &[1.0, 0.0, 0.0, 0.0], fields).unwrap();
        seg.tombstone("doc1");
        seg.seal();

        let persisted = seg.to_persisted().unwrap();
        let restored = VectorSegment::from_persisted(persisted).unwrap();

        assert_eq!(restored.id, seg.id);
        assert!(restored.sealed);
        assert!(!restored.contains("doc1")); // tombstoned
        assert_eq!(restored.total_count(), 1);
        assert_eq!(restored.deleted_count(), 1);
    }

    #[test]
    fn test_live_count() {
        let mut seg = make_segment(4);
        let fields: HashMap<String, serde_json::Value> = HashMap::new();
        seg.add("doc1", &[1.0, 0.0, 0.0, 0.0], fields.clone())
            .unwrap();
        seg.add("doc2", &[0.0, 1.0, 0.0, 0.0], fields).unwrap();

        assert_eq!(seg.live_count(), 2);
        assert_eq!(seg.total_count(), 2);
        assert_eq!(seg.deleted_count(), 0);

        seg.tombstone("doc1");
        assert_eq!(seg.live_count(), 1);
        assert_eq!(seg.deleted_count(), 1);
    }
}
