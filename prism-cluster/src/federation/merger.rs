//! Result merging strategies for federated search
//!
//! Implements various merge strategies optimized for different query types:
//! - Simple: Just concatenate and sort (for filters/exact match)
//! - Score normalized: Normalize scores across shards (for BM25)
//! - RRF: Reciprocal Rank Fusion (for hybrid search)

use crate::types::{RpcSearchResult, RpcSearchResults};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Strategy for merging search results from multiple shards
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    /// Simple merge: concatenate and sort by score
    /// Best for: filter queries, exact matches, vector search (already normalized)
    Simple,

    /// Score normalization: normalize scores across shards before merging
    /// Best for: BM25 text search where scores vary by shard
    ScoreNormalized,

    /// Reciprocal Rank Fusion: merge by rank position
    /// Best for: hybrid search combining multiple result sets
    ReciprocalRankFusion {
        /// RRF constant (default: 60)
        k: u32,
    },

    /// Two-phase merge for aggregations
    /// First phase: collect partial aggregations
    /// Second phase: merge partials into final
    TwoPhase,

    /// Weighted merge: apply weight per shard
    Weighted {
        /// Weight per shard (shard_id -> weight)
        weights: HashMap<String, f32>,
    },
}

impl Default for MergeStrategy {
    fn default() -> Self {
        MergeStrategy::Simple
    }
}

impl MergeStrategy {
    /// Parse from string (used in query)
    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "simple" => Some(MergeStrategy::Simple),
            "score_normalized" | "normalized" => Some(MergeStrategy::ScoreNormalized),
            "rrf" | "reciprocal_rank_fusion" => Some(MergeStrategy::ReciprocalRankFusion { k: 60 }),
            "two_phase" => Some(MergeStrategy::TwoPhase),
            _ => None,
        }
    }

    /// Get strategy name for metrics
    pub fn name(&self) -> &'static str {
        match self {
            MergeStrategy::Simple => "simple",
            MergeStrategy::ScoreNormalized => "score_normalized",
            MergeStrategy::ReciprocalRankFusion { .. } => "rrf",
            MergeStrategy::TwoPhase => "two_phase",
            MergeStrategy::Weighted { .. } => "weighted",
        }
    }
}

/// Result merger
pub struct ResultMerger {
    default_strategy: MergeStrategy,
}

impl ResultMerger {
    /// Create a new result merger
    pub fn new(default_strategy: MergeStrategy) -> Self {
        Self { default_strategy }
    }

    /// Merge results from multiple shards
    pub fn merge(
        &self,
        shard_results: Vec<RpcSearchResults>,
        limit: usize,
        strategy: &MergeStrategy,
    ) -> MergedResults {
        if shard_results.is_empty() {
            return MergedResults {
                results: Vec::new(),
                total: 0,
                strategy_used: strategy.clone(),
            };
        }

        match strategy {
            MergeStrategy::Simple => self.merge_simple(shard_results, limit),
            MergeStrategy::ScoreNormalized => self.merge_normalized(shard_results, limit),
            MergeStrategy::ReciprocalRankFusion { k } => self.merge_rrf(shard_results, limit, *k),
            MergeStrategy::TwoPhase => self.merge_simple(shard_results, limit), // Same as simple for now
            MergeStrategy::Weighted { weights } => {
                self.merge_weighted(shard_results, limit, weights)
            }
        }
    }

    /// Simple merge: concatenate and sort by score (descending)
    fn merge_simple(&self, shard_results: Vec<RpcSearchResults>, limit: usize) -> MergedResults {
        let total: usize = shard_results.iter().map(|r| r.total).sum();

        let mut all_results: Vec<RpcSearchResult> =
            shard_results.into_iter().flat_map(|r| r.results).collect();

        // Sort by score descending
        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Deduplicate by ID (keep highest score)
        let mut seen = std::collections::HashSet::new();
        all_results.retain(|r| seen.insert(r.id.clone()));

        // Apply limit
        all_results.truncate(limit);

        MergedResults {
            results: all_results,
            total,
            strategy_used: MergeStrategy::Simple,
        }
    }

    /// Score-normalized merge: normalize scores per shard before merging
    fn merge_normalized(
        &self,
        shard_results: Vec<RpcSearchResults>,
        limit: usize,
    ) -> MergedResults {
        let total: usize = shard_results.iter().map(|r| r.total).sum();

        let mut all_results: Vec<RpcSearchResult> = Vec::new();

        for shard_result in shard_results {
            let normalized = ScoreNormalizer::min_max_normalize(shard_result.results);
            all_results.extend(normalized);
        }

        // Sort by normalized score descending
        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Deduplicate
        let mut seen = std::collections::HashSet::new();
        all_results.retain(|r| seen.insert(r.id.clone()));

        all_results.truncate(limit);

        MergedResults {
            results: all_results,
            total,
            strategy_used: MergeStrategy::ScoreNormalized,
        }
    }

    /// Reciprocal Rank Fusion merge
    ///
    /// RRF score = sum(1 / (k + rank)) for each result list containing the doc
    fn merge_rrf(
        &self,
        shard_results: Vec<RpcSearchResults>,
        limit: usize,
        k: u32,
    ) -> MergedResults {
        let total: usize = shard_results.iter().map(|r| r.total).sum();

        // Calculate RRF scores
        let mut rrf_scores: HashMap<String, (f32, RpcSearchResult)> = HashMap::new();

        for shard_result in shard_results {
            for (rank, result) in shard_result.results.into_iter().enumerate() {
                let rrf_contribution = 1.0 / (k as f32 + rank as f32 + 1.0);

                rrf_scores
                    .entry(result.id.clone())
                    .and_modify(|(score, _)| *score += rrf_contribution)
                    .or_insert((rrf_contribution, result));
            }
        }

        // Convert to results with RRF scores
        let mut results: Vec<RpcSearchResult> = rrf_scores
            .into_iter()
            .map(|(_, (rrf_score, mut result))| {
                result.score = rrf_score;
                result
            })
            .collect();

        // Sort by RRF score descending
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(limit);

        MergedResults {
            results,
            total,
            strategy_used: MergeStrategy::ReciprocalRankFusion { k },
        }
    }

    /// Weighted merge: apply per-shard weights
    fn merge_weighted(
        &self,
        shard_results: Vec<RpcSearchResults>,
        limit: usize,
        _weights: &HashMap<String, f32>,
    ) -> MergedResults {
        // For now, same as simple merge
        // In full implementation, would apply weight multiplier per shard
        self.merge_simple(shard_results, limit)
    }
}

/// Score normalization utilities
pub struct ScoreNormalizer;

impl ScoreNormalizer {
    /// Min-max normalization to [0, 1] range
    pub fn min_max_normalize(mut results: Vec<RpcSearchResult>) -> Vec<RpcSearchResult> {
        if results.is_empty() {
            return results;
        }

        let min_score = results
            .iter()
            .map(|r| r.score)
            .fold(f32::INFINITY, f32::min);
        let max_score = results
            .iter()
            .map(|r| r.score)
            .fold(f32::NEG_INFINITY, f32::max);

        let range = max_score - min_score;

        if range > 0.0 {
            for result in &mut results {
                result.score = (result.score - min_score) / range;
            }
        } else {
            // All same score - normalize to 1.0
            for result in &mut results {
                result.score = 1.0;
            }
        }

        results
    }

    /// Z-score normalization
    pub fn z_score_normalize(mut results: Vec<RpcSearchResult>) -> Vec<RpcSearchResult> {
        if results.is_empty() {
            return results;
        }

        let n = results.len() as f32;
        let mean: f32 = results.iter().map(|r| r.score).sum::<f32>() / n;
        let variance: f32 = results
            .iter()
            .map(|r| (r.score - mean).powi(2))
            .sum::<f32>()
            / n;
        let std_dev = variance.sqrt();

        if std_dev > 0.0 {
            for result in &mut results {
                result.score = (result.score - mean) / std_dev;
            }
        }

        results
    }
}

/// Result of merging
pub struct MergedResults {
    pub results: Vec<RpcSearchResult>,
    pub total: usize,
    pub strategy_used: MergeStrategy,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_result(id: &str, score: f32) -> RpcSearchResult {
        RpcSearchResult {
            id: id.to_string(),
            score,
            fields: HashMap::new(),
            highlight: None,
        }
    }

    fn make_shard_results(results: Vec<RpcSearchResult>) -> RpcSearchResults {
        RpcSearchResults {
            results,
            total: 100, // Arbitrary
            latency_ms: 10,
        }
    }

    #[test]
    fn test_merge_simple() {
        let merger = ResultMerger::new(MergeStrategy::Simple);

        let shard1 = make_shard_results(vec![make_result("doc-1", 0.9), make_result("doc-2", 0.7)]);
        let shard2 =
            make_shard_results(vec![make_result("doc-3", 0.85), make_result("doc-4", 0.6)]);

        let merged = merger.merge(vec![shard1, shard2], 10, &MergeStrategy::Simple);

        assert_eq!(merged.results.len(), 4);
        assert_eq!(merged.results[0].id, "doc-1"); // Highest score
        assert_eq!(merged.results[1].id, "doc-3");
    }

    #[test]
    fn test_merge_rrf() {
        let merger = ResultMerger::new(MergeStrategy::Simple);

        // Same doc appears in both shards at different ranks
        let shard1 = make_shard_results(vec![make_result("doc-1", 0.9), make_result("doc-2", 0.7)]);
        let shard2 = make_shard_results(vec![
            make_result("doc-2", 0.85), // doc-2 is #1 here
            make_result("doc-3", 0.6),
        ]);

        let merged = merger.merge(
            vec![shard1, shard2],
            10,
            &MergeStrategy::ReciprocalRankFusion { k: 60 },
        );

        // doc-2 appears in both, should have higher RRF score
        assert_eq!(merged.results.len(), 3); // 3 unique docs
                                             // doc-2 should be first (appears in both lists)
        assert_eq!(merged.results[0].id, "doc-2");
    }

    #[test]
    fn test_merge_deduplication() {
        let merger = ResultMerger::new(MergeStrategy::Simple);

        let shard1 = make_shard_results(vec![make_result("doc-1", 0.9)]);
        let shard2 = make_shard_results(vec![
            make_result("doc-1", 0.85), // Duplicate with lower score
        ]);

        let merged = merger.merge(vec![shard1, shard2], 10, &MergeStrategy::Simple);

        assert_eq!(merged.results.len(), 1);
        assert_eq!(merged.results[0].score, 0.9); // Higher score kept
    }

    #[test]
    fn test_score_normalization() {
        let results = vec![
            make_result("doc-1", 100.0),
            make_result("doc-2", 50.0),
            make_result("doc-3", 0.0),
        ];

        let normalized = ScoreNormalizer::min_max_normalize(results);

        assert_eq!(normalized[0].score, 1.0);
        assert_eq!(normalized[1].score, 0.5);
        assert_eq!(normalized[2].score, 0.0);
    }

    #[test]
    fn test_merge_strategy_from_string() {
        assert_eq!(
            MergeStrategy::from_string("simple"),
            Some(MergeStrategy::Simple)
        );
        assert_eq!(
            MergeStrategy::from_string("rrf"),
            Some(MergeStrategy::ReciprocalRankFusion { k: 60 })
        );
        assert_eq!(
            MergeStrategy::from_string("normalized"),
            Some(MergeStrategy::ScoreNormalized)
        );
        assert_eq!(MergeStrategy::from_string("unknown"), None);
    }
}
