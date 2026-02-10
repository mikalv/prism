//! Vector shard: manages an active segment and sealed segments.
//!
//! Each shard holds one active (writable) segment and zero or more sealed
//! (immutable, searchable) segments. Documents are assigned to shards by
//! hashing their ID.

use crate::backends::r#trait::SearchResult;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::index::Metric;
use super::segment::{PersistedSegment, SegmentId, VectorSegment};

/// A single vector shard managing one active segment and sealed segments.
pub struct VectorShard {
    pub shard_id: u32,
    pub active_segment: VectorSegment,
    pub sealed_segments: Vec<VectorSegment>,
    pub dimensions: usize,
    pub metric: Metric,
    pub m: usize,
    pub ef_construction: usize,
    pub ef_search: usize,
    pub embedding_source_field: Option<String>,
    pub embedding_target_field: String,
    pub next_segment_id: SegmentId,
}

/// Serializable shard state.
#[derive(Serialize, Deserialize)]
pub struct PersistedShard {
    pub shard_id: u32,
    pub dimensions: usize,
    pub metric: Metric,
    pub m: usize,
    pub ef_construction: usize,
    pub ef_search: usize,
    pub embedding_source_field: Option<String>,
    pub embedding_target_field: String,
    pub active_segment: PersistedSegment,
    pub sealed_segments: Vec<PersistedSegment>,
    pub next_segment_id: SegmentId,
}

impl VectorShard {
    /// Create a new shard with a fresh active segment.
    pub fn new(
        shard_id: u32,
        dimensions: usize,
        metric: Metric,
        m: usize,
        ef_construction: usize,
        ef_search: usize,
        embedding_source_field: Option<String>,
        embedding_target_field: String,
    ) -> Result<Self> {
        let active = VectorSegment::new(0, dimensions, metric, m, ef_construction)?;
        Ok(Self {
            shard_id,
            active_segment: active,
            sealed_segments: Vec::new(),
            dimensions,
            metric,
            m,
            ef_construction,
            ef_search,
            embedding_source_field,
            embedding_target_field,
            next_segment_id: 1,
        })
    }

    /// Index a document into the active segment.
    pub fn index(
        &mut self,
        doc_id: &str,
        vector: &[f32],
        fields: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        // If doc already exists anywhere in this shard, tombstone the old copy
        self.delete_if_exists(doc_id);
        self.active_segment.add(doc_id, vector, fields)
    }

    /// Search all segments (active + sealed) and merge results by score.
    pub fn search(&self, query_vector: &[f32], k: usize) -> Result<Vec<SearchResult>> {
        let oversample_k = k; // oversample is applied at the ShardedVectorIndex level
        let mut all_results = Vec::new();

        // Search active segment
        let active_results = self
            .active_segment
            .search(query_vector, oversample_k, self.ef_search)?;
        all_results.extend(active_results);

        // Search sealed segments
        for seg in &self.sealed_segments {
            let seg_results = seg.search(query_vector, oversample_k, self.ef_search)?;
            all_results.extend(seg_results);
        }

        // Sort by score descending and dedup by id (prefer higher score)
        all_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Dedup: keep first occurrence (highest score) per doc_id
        let mut seen = std::collections::HashSet::new();
        all_results.retain(|r| seen.insert(r.id.clone()));

        all_results.truncate(k);
        Ok(all_results)
    }

    /// Get a document by ID from any segment.
    pub fn get(&self, doc_id: &str) -> Option<HashMap<String, serde_json::Value>> {
        if let Some(fields) = self.active_segment.get(doc_id) {
            return Some(fields.clone());
        }
        for seg in &self.sealed_segments {
            if let Some(fields) = seg.get(doc_id) {
                return Some(fields.clone());
            }
        }
        None
    }

    /// Check if this shard contains the document.
    pub fn contains(&self, doc_id: &str) -> bool {
        if self.active_segment.contains(doc_id) {
            return true;
        }
        self.sealed_segments.iter().any(|s| s.contains(doc_id))
    }

    /// Delete a document by tombstoning it across all segments.
    pub fn delete(&mut self, doc_id: &str) -> bool {
        self.delete_if_exists(doc_id)
    }

    fn delete_if_exists(&mut self, doc_id: &str) -> bool {
        let mut found = false;
        if self.active_segment.tombstone(doc_id) {
            found = true;
        }
        for seg in &mut self.sealed_segments {
            if seg.tombstone(doc_id) {
                found = true;
            }
        }
        found
    }

    /// Seal the current active segment and create a new one.
    pub fn seal_active(&mut self) -> Result<()> {
        let new_active = VectorSegment::new(
            self.next_segment_id,
            self.dimensions,
            self.metric,
            self.m,
            self.ef_construction,
        )?;
        self.next_segment_id += 1;

        let old_active = std::mem::replace(&mut self.active_segment, new_active);
        // Only seal and keep if it has data
        if old_active.total_count() > 0 {
            let mut sealed = old_active;
            sealed.seal();
            self.sealed_segments.push(sealed);
        }
        Ok(())
    }

    /// Count of live documents across all segments.
    pub fn live_count(&self) -> u64 {
        let active = self.active_segment.live_count();
        let sealed: u64 = self.sealed_segments.iter().map(|s| s.live_count()).sum();
        active + sealed
    }

    /// Total document count (including tombstoned) across all segments.
    pub fn total_count(&self) -> u64 {
        let active = self.active_segment.total_count();
        let sealed: u64 = self.sealed_segments.iter().map(|s| s.total_count()).sum();
        active + sealed
    }

    /// Estimated size in bytes.
    pub fn estimated_size(&self, dimension: usize) -> usize {
        let doc_count = self.live_count() as usize;
        let vector_size = dimension * 4 * doc_count;
        let metadata_size = self.all_documents_metadata_size();
        vector_size + metadata_size
    }

    fn all_documents_metadata_size(&self) -> usize {
        let mut size = self.segment_metadata_size(&self.active_segment);
        for seg in &self.sealed_segments {
            size += self.segment_metadata_size(seg);
        }
        size
    }

    fn segment_metadata_size(&self, seg: &VectorSegment) -> usize {
        seg.documents
            .iter()
            .filter(|(id, _)| seg.contains(id))
            .map(|(_, fields)| {
                fields
                    .iter()
                    .map(|(k, v)| k.len() + v.to_string().len())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Serialize for persistence.
    pub fn to_persisted(&self) -> Result<PersistedShard> {
        let active = self.active_segment.to_persisted()?;
        let mut sealed = Vec::new();
        for seg in &self.sealed_segments {
            sealed.push(seg.to_persisted()?);
        }
        Ok(PersistedShard {
            shard_id: self.shard_id,
            dimensions: self.dimensions,
            metric: self.metric,
            m: self.m,
            ef_construction: self.ef_construction,
            ef_search: self.ef_search,
            embedding_source_field: self.embedding_source_field.clone(),
            embedding_target_field: self.embedding_target_field.clone(),
            active_segment: active,
            sealed_segments: sealed,
            next_segment_id: self.next_segment_id,
        })
    }

    /// Restore from persisted state.
    pub fn from_persisted(p: PersistedShard) -> Result<Self> {
        let active = VectorSegment::from_persisted(p.active_segment)?;
        let mut sealed = Vec::new();
        for seg_p in p.sealed_segments {
            sealed.push(VectorSegment::from_persisted(seg_p)?);
        }
        Ok(Self {
            shard_id: p.shard_id,
            active_segment: active,
            sealed_segments: sealed,
            dimensions: p.dimensions,
            metric: p.metric,
            m: p.m,
            ef_construction: p.ef_construction,
            ef_search: p.ef_search,
            embedding_source_field: p.embedding_source_field,
            embedding_target_field: p.embedding_target_field,
            next_segment_id: p.next_segment_id,
        })
    }
}

/// Compute shard assignment for a document ID.
pub fn shard_for_doc(doc_id: &str, num_shards: usize) -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    doc_id.hash(&mut hasher);
    (hasher.finish() % num_shards as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shard() -> VectorShard {
        VectorShard::new(
            0,
            4,
            Metric::Cosine,
            16,
            200,
            100,
            None,
            "embedding".to_string(),
        )
        .unwrap()
    }

    #[test]
    fn test_index_and_search() {
        let mut shard = make_shard();
        let fields = HashMap::new();
        shard
            .index("doc1", &[1.0, 0.0, 0.0, 0.0], fields.clone())
            .unwrap();
        shard
            .index("doc2", &[0.0, 1.0, 0.0, 0.0], fields)
            .unwrap();

        let results = shard.search(&[1.0, 0.0, 0.0, 0.0], 2).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "doc1");
    }

    #[test]
    fn test_delete() {
        let mut shard = make_shard();
        shard
            .index("doc1", &[1.0, 0.0, 0.0, 0.0], HashMap::new())
            .unwrap();
        assert!(shard.contains("doc1"));

        shard.delete("doc1");
        assert!(!shard.contains("doc1"));
    }

    #[test]
    fn test_seal_and_search_across_segments() {
        let mut shard = make_shard();
        shard
            .index("doc1", &[1.0, 0.0, 0.0, 0.0], HashMap::new())
            .unwrap();
        shard.seal_active().unwrap();
        shard
            .index("doc2", &[0.0, 1.0, 0.0, 0.0], HashMap::new())
            .unwrap();

        assert_eq!(shard.sealed_segments.len(), 1);
        let results = shard.search(&[1.0, 0.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_shard_assignment_distribution() {
        let num_shards = 4;
        let mut counts = vec![0u32; num_shards];
        for i in 0..1000 {
            let doc_id = format!("doc_{}", i);
            let shard = shard_for_doc(&doc_id, num_shards);
            counts[shard as usize] += 1;
        }
        // Each shard should get roughly 250 docs (within reasonable bounds)
        for count in &counts {
            assert!(*count > 150, "Shard got too few docs: {}", count);
            assert!(*count < 350, "Shard got too many docs: {}", count);
        }
    }

    #[test]
    fn test_shard_persistence_roundtrip() {
        let mut shard = make_shard();
        shard
            .index("doc1", &[1.0, 0.0, 0.0, 0.0], HashMap::new())
            .unwrap();
        shard.seal_active().unwrap();
        shard
            .index("doc2", &[0.0, 1.0, 0.0, 0.0], HashMap::new())
            .unwrap();

        let persisted = shard.to_persisted().unwrap();
        let restored = VectorShard::from_persisted(persisted).unwrap();

        assert_eq!(restored.shard_id, 0);
        assert_eq!(restored.sealed_segments.len(), 1);
        assert!(restored.contains("doc1"));
        assert!(restored.contains("doc2"));
    }

    #[test]
    fn test_reindex_replaces_old_doc() {
        let mut shard = make_shard();
        shard
            .index("doc1", &[1.0, 0.0, 0.0, 0.0], HashMap::new())
            .unwrap();
        // Re-index same doc with different vector
        shard
            .index("doc1", &[0.0, 1.0, 0.0, 0.0], HashMap::new())
            .unwrap();

        assert_eq!(shard.live_count(), 1);
    }
}
