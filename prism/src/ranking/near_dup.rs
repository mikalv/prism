//! Semantic near-duplicate collapse — drop results whose embedding is too
//! similar to a higher-scoring result already kept (greedy diversification).
//!
//! Complements exact field collapse (`ranking::collapse`): instead of grouping
//! on a shared field value, results are grouped by embedding proximity, so a
//! page won't be dominated by many rephrasings of the same content.

use crate::backends::SearchResult;

/// Collapse score-ordered `results`, dropping any hit whose cosine similarity to
/// an already-kept hit is `>= threshold`. Input order is preserved, so the
/// highest-scoring member of each near-duplicate cluster is the one kept.
///
/// The comparison vector is read from `results[i].fields[vector_field]`, which
/// must be a JSON array of numbers. Hits without a usable vector are always kept
/// (they cannot be compared).
pub fn collapse_near_duplicates(
    results: Vec<SearchResult>,
    vector_field: &str,
    threshold: f32,
) -> Vec<SearchResult> {
    let mut kept_vectors: Vec<Vec<f32>> = Vec::new();
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        match extract_vector(&r, vector_field) {
            Some(v) => {
                let is_dup = kept_vectors
                    .iter()
                    .any(|k| cosine_similarity(&v, k) >= threshold);
                if !is_dup {
                    kept_vectors.push(v);
                    out.push(r);
                }
                // otherwise: near-duplicate of a higher-scoring hit → drop
            }
            // No usable vector → cannot compare; always keep.
            None => out.push(r),
        }
    }
    out
}

/// Extract a `Vec<f32>` embedding from a result field, if present and well-formed.
fn extract_vector(result: &SearchResult, field: &str) -> Option<Vec<f32>> {
    let arr = result.fields.get(field)?.as_array()?;
    let v: Vec<f32> = arr.iter().filter_map(|x| x.as_f64().map(|n| n as f32)).collect();
    if v.len() == arr.len() && !v.is_empty() {
        Some(v)
    } else {
        None
    }
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
    use serde_json::json;
    use std::collections::HashMap;

    fn hit(id: &str, score: f32, vector: Option<Vec<f32>>) -> SearchResult {
        let mut fields = HashMap::new();
        if let Some(v) = vector {
            fields.insert("embedding".to_string(), json!(v));
        }
        SearchResult {
            id: id.into(),
            score,
            fields,
            highlight: None,
        }
    }

    #[test]
    fn drops_near_duplicate_of_higher_scoring_hit() {
        let results = vec![
            hit("a", 5.0, Some(vec![1.0, 0.0])),
            hit("b", 4.0, Some(vec![0.99, 0.01])), // ~identical to a → dropped
            hit("c", 3.0, Some(vec![0.0, 1.0])),   // orthogonal → kept
        ];

        let out = collapse_near_duplicates(results, "embedding", 0.9);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c"]);
    }

    #[test]
    fn keeps_distinct_hits_below_threshold() {
        let results = vec![
            hit("a", 5.0, Some(vec![1.0, 0.0])),
            hit("b", 4.0, Some(vec![0.7, 0.7])), // sim ~0.707 < 0.9 → kept
        ];

        let out = collapse_near_duplicates(results, "embedding", 0.9);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn keeps_hits_without_a_vector() {
        let results = vec![
            hit("a", 5.0, Some(vec![1.0, 0.0])),
            hit("x", 4.0, None), // no embedding → cannot compare → kept
            hit("b", 3.0, Some(vec![1.0, 0.0])), // duplicate of a → dropped
        ];

        let out = collapse_near_duplicates(results, "embedding", 0.9);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "x"]);
    }

    #[test]
    fn compares_against_all_kept_not_just_the_last() {
        // Kept set becomes [a, d] (orthogonal). `e` is a near-duplicate of `a`
        // but orthogonal to `d` (the most recently kept). It must still drop,
        // proving we compare against every kept hit, not just the previous one.
        let results = vec![
            hit("a", 5.0, Some(vec![1.0, 0.0])),
            hit("d", 4.0, Some(vec![0.0, 1.0])),
            hit("e", 3.0, Some(vec![0.98, 0.02])), // dup of a, not of d → dropped
        ];

        let out = collapse_near_duplicates(results, "embedding", 0.9);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "d"]);
    }
}
