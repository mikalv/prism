//! Semantic near-duplicate collapse — drop results whose embedding is too
//! similar to a higher-scoring result already kept (greedy diversification).
//!
//! Complements exact field collapse (`ranking::collapse`): instead of grouping
//! on a shared field value, results are grouped by embedding proximity, so a
//! page won't be dominated by many rephrasings of the same content.
//!
//! Result embeddings are not carried on `SearchResult`; the caller supplies an
//! `id -> vector` map (fetched from the vector backend for the returned hits).

use crate::backends::SearchResult;
use serde::Deserialize;
use std::collections::HashMap;

/// Configuration for semantic near-duplicate collapse.
#[derive(Debug, Clone, Deserialize)]
pub struct NearDupConfig {
    /// Cosine similarity at or above which two hits are treated as duplicates
    /// (default: 0.95).
    #[serde(default = "default_threshold")]
    pub threshold: f32,
}

fn default_threshold() -> f32 {
    0.95
}

/// Collapse score-ordered `results`, dropping any hit whose cosine similarity to
/// an already-kept hit is `>= threshold`. Input order is preserved, so the
/// highest-scoring member of each near-duplicate cluster is the one kept.
///
/// The comparison vector for a hit is looked up in `vectors` by result id. Hits
/// with no vector in the map are always kept (they cannot be compared).
pub fn collapse_near_duplicates(
    results: Vec<SearchResult>,
    vectors: &HashMap<String, Vec<f32>>,
    threshold: f32,
) -> Vec<SearchResult> {
    let mut kept: Vec<&Vec<f32>> = Vec::new();
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        match vectors.get(&r.id) {
            Some(v) => {
                let is_dup = kept.iter().any(|k| cosine_similarity(v, k) >= threshold);
                if !is_dup {
                    kept.push(v);
                    out.push(r);
                }
                // otherwise: near-duplicate of a higher-scoring hit → drop
            }
            // No vector available → cannot compare; always keep.
            None => out.push(r),
        }
    }
    out
}

/// Cosine similarity of two equal-length vectors. Returns 0.0 when the lengths
/// differ or either vector has zero magnitude (nothing meaningful to compare).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, score: f32) -> SearchResult {
        SearchResult {
            id: id.into(),
            score,
            fields: HashMap::new(),
            highlight: None,
            score_explanation: None,
        }
    }

    fn vecs(pairs: &[(&str, Vec<f32>)]) -> HashMap<String, Vec<f32>> {
        pairs
            .iter()
            .map(|(id, v)| (id.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn drops_near_duplicate_of_higher_scoring_hit() {
        let results = vec![hit("a", 5.0), hit("b", 4.0), hit("c", 3.0)];
        let vectors = vecs(&[
            ("a", vec![1.0, 0.0]),
            ("b", vec![0.99, 0.01]), // ~identical to a → dropped
            ("c", vec![0.0, 1.0]),   // orthogonal → kept
        ]);

        let out = collapse_near_duplicates(results, &vectors, 0.9);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c"]);
    }

    #[test]
    fn keeps_distinct_hits_below_threshold() {
        let results = vec![hit("a", 5.0), hit("b", 4.0)];
        let vectors = vecs(&[
            ("a", vec![1.0, 0.0]),
            ("b", vec![0.7, 0.7]), // sim ~0.707 < 0.9 → kept
        ]);

        let out = collapse_near_duplicates(results, &vectors, 0.9);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn keeps_hits_without_a_vector() {
        let results = vec![hit("a", 5.0), hit("x", 4.0), hit("b", 3.0)];
        // "x" has no entry in the vectors map → cannot compare → kept.
        let vectors = vecs(&[
            ("a", vec![1.0, 0.0]),
            ("b", vec![1.0, 0.0]), // duplicate of a → dropped
        ]);

        let out = collapse_near_duplicates(results, &vectors, 0.9);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "x"]);
    }

    #[test]
    fn compares_against_all_kept_not_just_the_last() {
        // Kept set becomes [a, d] (orthogonal). `e` is a near-duplicate of `a`
        // but orthogonal to `d` (the most recently kept). It must still drop,
        // proving we compare against every kept hit, not just the previous one.
        let results = vec![hit("a", 5.0), hit("d", 4.0), hit("e", 3.0)];
        let vectors = vecs(&[
            ("a", vec![1.0, 0.0]),
            ("d", vec![0.0, 1.0]),
            ("e", vec![0.98, 0.02]), // dup of a, not of d → dropped
        ]);

        let out = collapse_near_duplicates(results, &vectors, 0.9);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "d"]);
    }
}
