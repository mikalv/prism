//! Segment compaction: merges sealed segments to reclaim space from tombstones.

use crate::error::Result;
use crate::schema::types::VectorCompactionConfig;

use super::segment::VectorSegment;
use super::shard::VectorShard;

/// Compact a shard by merging sealed segments with high delete ratios.
///
/// Returns the number of segments that were compacted (replaced).
pub fn compact_shard(shard: &mut VectorShard, config: &VectorCompactionConfig) -> Result<usize> {
    if shard.sealed_segments.len() < config.min_segments {
        return Ok(0);
    }

    // Find candidates for compaction
    let candidates: Vec<usize> = shard
        .sealed_segments
        .iter()
        .enumerate()
        .filter(|(_, seg)| seg.is_compaction_candidate(config.delete_ratio_threshold))
        .map(|(i, _)| i)
        .collect();

    if candidates.is_empty() {
        return Ok(0);
    }

    // Build a new segment from live data in candidate segments
    let new_segment_id = shard
        .sealed_segments
        .iter()
        .map(|s| s.id)
        .max()
        .unwrap_or(0)
        + 1;

    let mut new_segment = VectorSegment::new(
        new_segment_id,
        shard.dimensions,
        shard.metric,
        shard.m,
        shard.ef_construction,
    )?;

    // Copy live data from candidate segments into new segment
    for &idx in &candidates {
        let seg = &shard.sealed_segments[idx];
        for (doc_id, &key) in &seg.id_to_key {
            if seg.tombstones.contains(key) {
                continue;
            }
            if let Some(fields) = seg.documents.get(doc_id) {
                // Get the vector by searching for the key specifically
                // Since we can't extract vectors directly from HNSW, we store them
                // in the fields map. For compaction, we need the original vector.
                // The vector is stored in the document fields under the target field.
                if let Some(vector_value) = fields.get(&shard.embedding_target_field) {
                    if let Ok(vector) = serde_json::from_value::<Vec<f32>>(vector_value.clone()) {
                        new_segment.add(doc_id, &vector, fields.clone())?;
                    }
                }
            }
        }
    }

    new_segment.seal();

    // Remove old candidate segments (iterate in reverse to maintain indices)
    let compacted_count = candidates.len();
    for &idx in candidates.iter().rev() {
        shard.sealed_segments.remove(idx);
    }

    // Add the new compacted segment if it has data
    if new_segment.total_count() > 0 {
        shard.sealed_segments.push(new_segment);
    }

    tracing::info!(
        shard_id = shard.shard_id,
        compacted = compacted_count,
        "Compacted sealed segments"
    );

    Ok(compacted_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::vector::index::Metric;
    use crate::backends::vector::shard::VectorShard;
    use std::collections::HashMap;

    fn make_shard_with_docs(doc_count: usize) -> VectorShard {
        let mut shard = VectorShard::new(
            0,
            4,
            Metric::Cosine,
            16,
            200,
            100,
            None,
            "embedding".to_string(),
        )
        .unwrap();

        for i in 0..doc_count {
            let mut fields = HashMap::new();
            fields.insert(
                "embedding".to_string(),
                serde_json::to_value(vec![i as f32, 0.0, 0.0, 0.0]).unwrap(),
            );
            shard
                .index(&format!("doc{}", i), &[i as f32, 0.0, 0.0, 0.0], fields)
                .unwrap();
        }

        shard
    }

    #[test]
    fn test_no_compaction_below_min_segments() {
        let mut shard = make_shard_with_docs(10);
        shard.seal_active().unwrap();
        // Only 1 sealed segment, min_segments=3
        let config = VectorCompactionConfig {
            min_segments: 3,
            delete_ratio_threshold: 0.2,
        };
        let compacted = compact_shard(&mut shard, &config).unwrap();
        assert_eq!(compacted, 0);
    }

    #[test]
    fn test_compaction_merges_segments() {
        let mut shard = VectorShard::new(
            0,
            4,
            Metric::Cosine,
            16,
            200,
            100,
            None,
            "embedding".to_string(),
        )
        .unwrap();

        // Create 3 sealed segments, each with some docs
        for batch in 0..3 {
            for i in 0..5 {
                let doc_id = format!("batch{}_doc{}", batch, i);
                let mut fields = HashMap::new();
                let vec = vec![(batch * 5 + i) as f32, 0.0, 0.0, 0.0];
                fields.insert("embedding".to_string(), serde_json::to_value(&vec).unwrap());
                shard.index(&doc_id, &vec, fields).unwrap();
            }
            shard.seal_active().unwrap();
        }

        assert_eq!(shard.sealed_segments.len(), 3);

        // Tombstone >20% of each segment
        for batch in 0..3 {
            shard.delete(&format!("batch{}_doc0", batch));
            shard.delete(&format!("batch{}_doc1", batch));
        }

        let config = VectorCompactionConfig {
            min_segments: 2,
            delete_ratio_threshold: 0.2,
        };

        let compacted = compact_shard(&mut shard, &config).unwrap();
        assert!(compacted > 0);

        // After compaction, no tombstoned docs should remain in new segments
        for batch in 0..3 {
            assert!(!shard.contains(&format!("batch{}_doc0", batch)));
            assert!(!shard.contains(&format!("batch{}_doc1", batch)));
        }

        // Live docs should still be searchable
        for batch in 0..3 {
            for i in 2..5 {
                assert!(shard.contains(&format!("batch{}_doc{}", batch, i)));
            }
        }
    }

    #[test]
    fn test_no_compaction_without_deletes() {
        let mut shard = VectorShard::new(
            0,
            4,
            Metric::Cosine,
            16,
            200,
            100,
            None,
            "embedding".to_string(),
        )
        .unwrap();

        for batch in 0..3 {
            for i in 0..5 {
                let doc_id = format!("batch{}_doc{}", batch, i);
                let mut fields = HashMap::new();
                let vec = vec![(batch * 5 + i) as f32, 0.0, 0.0, 0.0];
                fields.insert("embedding".to_string(), serde_json::to_value(&vec).unwrap());
                shard.index(&doc_id, &vec, fields).unwrap();
            }
            shard.seal_active().unwrap();
        }

        let config = VectorCompactionConfig {
            min_segments: 2,
            delete_ratio_threshold: 0.2,
        };

        let compacted = compact_shard(&mut shard, &config).unwrap();
        assert_eq!(compacted, 0);
    }
}
