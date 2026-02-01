//! Ranking and relevance scoring module
//!
//! This module provides score adjustments for search results based on:
//! - Field boosting: weight certain fields higher than others
//! - Recency decay: boost newer documents over older ones
//! - Popularity boost: multiply scores by document-level boost values

pub mod decay;

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

pub use decay::{DecayConfig, DecayFunction, compute_decay, compute_decay_from_micros, parse_duration};

use crate::schema::BoostingConfig;

/// Score adjustment configuration derived from schema BoostingConfig
#[derive(Debug, Clone)]
pub struct RankingConfig {
    /// Field weights for boosting (field_name -> multiplier)
    pub field_weights: HashMap<String, f32>,
    /// Recency decay configuration
    pub recency_decay: Option<DecayConfig>,
    /// Custom ranking signals: (field_name, weight)
    pub signals: Vec<(String, f32)>,
}

impl RankingConfig {
    /// Create ranking config from schema's BoostingConfig
    pub fn from_boosting_config(config: &BoostingConfig) -> Self {
        let recency_decay = config.recency.as_ref().map(|r| {
            let function = DecayFunction::from_str(&r.decay_function);
            let scale = parse_duration(&r.scale).unwrap_or(Duration::from_secs(7 * 86400));
            let offset = r.offset.as_ref().and_then(|s| parse_duration(s));

            let mut decay_config = DecayConfig::new(function, scale, r.decay_rate as f64);
            if let Some(offset) = offset {
                decay_config = decay_config.with_offset(offset);
            }
            decay_config
        });

        let signals = config.signals.iter()
            .map(|s| (s.name.clone(), s.weight))
            .collect();

        Self {
            field_weights: config.field_weights.clone(),
            recency_decay,
            signals,
        }
    }
}

/// Apply ranking adjustments to search results
///
/// This function modifies scores based on:
/// 1. Recency decay - reduce scores for older documents
/// 2. Popularity boost - multiply by document's _boost value
///
/// Note: Field boosting is applied at query time, not post-processing.
///
/// # Arguments
/// * `results` - Search results with id, score, and fields
/// * `config` - Ranking configuration with decay settings
/// * `now` - Current time for recency calculations
///
/// # Returns
/// Results sorted by adjusted scores (highest first)
pub fn apply_ranking_adjustments(
    results: &mut Vec<RankableResult>,
    config: &RankingConfig,
    now: SystemTime,
) {
    for result in results.iter_mut() {
        let mut score = result.score as f64;

        // Apply recency decay if configured
        if let Some(decay_config) = &config.recency_decay {
            if let Some(indexed_at_micros) = result.indexed_at_micros {
                let decay = compute_decay_from_micros(decay_config, indexed_at_micros, now);
                score *= decay;
            }
        }

        // Apply document boost if present
        if let Some(boost) = result.boost {
            score *= boost;
        }

        // Apply custom ranking signals: each contributes field_value * weight
        for (field_name, weight) in &config.signals {
            if let Some(val) = result.fields.get(field_name) {
                let numeric = val.as_f64()
                    .or_else(|| val.as_i64().map(|i| i as f64))
                    .or_else(|| val.as_u64().map(|u| u as f64));
                if let Some(v) = numeric {
                    score += v * (*weight as f64);
                }
            }
        }

        result.adjusted_score = score as f32;
    }

    // Re-sort by adjusted score (highest first)
    results.sort_by(|a, b| {
        b.adjusted_score
            .partial_cmp(&a.adjusted_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// A search result that can have ranking adjustments applied
#[derive(Debug, Clone)]
pub struct RankableResult {
    pub id: String,
    pub score: f32,
    pub adjusted_score: f32,
    pub fields: HashMap<String, serde_json::Value>,
    /// _indexed_at timestamp in microseconds since epoch
    pub indexed_at_micros: Option<i64>,
    /// _boost value from document
    pub boost: Option<f64>,
}

impl RankableResult {
    /// Create from search result fields
    pub fn from_fields(
        id: String,
        score: f32,
        fields: HashMap<String, serde_json::Value>,
    ) -> Self {
        // Extract _indexed_at timestamp (stored as microseconds)
        let indexed_at_micros = fields
            .get("_indexed_at")
            .and_then(|v| v.as_i64());

        // Extract _boost value
        let boost = fields
            .get("_boost")
            .and_then(|v| v.as_f64());

        Self {
            id,
            score,
            adjusted_score: score, // Initially same as score
            fields,
            indexed_at_micros,
            boost,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(id: &str, score: f32, indexed_at_micros: Option<i64>, boost: Option<f64>) -> RankableResult {
        let mut fields = HashMap::new();
        if let Some(ts) = indexed_at_micros {
            fields.insert("_indexed_at".to_string(), serde_json::json!(ts));
        }
        if let Some(b) = boost {
            fields.insert("_boost".to_string(), serde_json::json!(b));
        }
        RankableResult::from_fields(id.to_string(), score, fields)
    }

    #[test]
    fn test_popularity_boost() {
        let config = RankingConfig {
            field_weights: HashMap::new(),
            recency_decay: None,
            signals: vec![],
        };

        let now = SystemTime::now();
        let mut results = vec![
            make_result("doc1", 1.0, None, Some(1.0)),
            make_result("doc2", 1.0, None, Some(2.0)),
            make_result("doc3", 1.0, None, Some(0.5)),
        ];

        apply_ranking_adjustments(&mut results, &config, now);

        // doc2 with boost=2.0 should be first
        assert_eq!(results[0].id, "doc2");
        assert!((results[0].adjusted_score - 2.0).abs() < 0.001);

        // doc1 with boost=1.0 should be second
        assert_eq!(results[1].id, "doc1");
        assert!((results[1].adjusted_score - 1.0).abs() < 0.001);

        // doc3 with boost=0.5 should be last
        assert_eq!(results[2].id, "doc3");
        assert!((results[2].adjusted_score - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_recency_decay() {
        use std::time::Duration;

        let config = RankingConfig {
            field_weights: HashMap::new(),
            signals: vec![],
            recency_decay: Some(DecayConfig::new(
                DecayFunction::Exponential,
                Duration::from_secs(7 * 86400), // 7 days
                0.5,
            )),
        };

        let now = SystemTime::now();
        let now_micros = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as i64;

        // Document from now (no decay)
        let recent_micros = now_micros;
        // Document from 7 days ago (should be ~0.5x)
        let week_old_micros = now_micros - (7 * 86400 * 1_000_000);
        // Document from 14 days ago (should be ~0.25x)
        let two_weeks_micros = now_micros - (14 * 86400 * 1_000_000);

        let mut results = vec![
            make_result("recent", 1.0, Some(recent_micros), None),
            make_result("week_old", 1.0, Some(week_old_micros), None),
            make_result("two_weeks", 1.0, Some(two_weeks_micros), None),
        ];

        apply_ranking_adjustments(&mut results, &config, now);

        // Recent should be first (highest score)
        assert_eq!(results[0].id, "recent");
        assert!(results[0].adjusted_score > 0.9);

        // Week old should be ~0.5
        let week_result = results.iter().find(|r| r.id == "week_old").unwrap();
        assert!((week_result.adjusted_score - 0.5).abs() < 0.1);

        // Two weeks should be ~0.25
        let two_weeks_result = results.iter().find(|r| r.id == "two_weeks").unwrap();
        assert!((two_weeks_result.adjusted_score - 0.25).abs() < 0.1);
    }

    #[test]
    fn test_combined_ranking() {
        use std::time::Duration;

        let config = RankingConfig {
            field_weights: HashMap::new(),
            signals: vec![],
            recency_decay: Some(DecayConfig::new(
                DecayFunction::Exponential,
                Duration::from_secs(7 * 86400),
                0.5,
            )),
        };

        let now = SystemTime::now();
        let now_micros = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as i64;

        // Old doc with high boost vs new doc with no boost
        let mut results = vec![
            make_result("old_popular", 1.0, Some(now_micros - 7 * 86400 * 1_000_000), Some(3.0)),
            make_result("new_regular", 1.0, Some(now_micros), Some(1.0)),
        ];

        apply_ranking_adjustments(&mut results, &config, now);

        // Old doc: 1.0 * 0.5 (decay) * 3.0 (boost) = 1.5
        // New doc: 1.0 * 1.0 (no decay) * 1.0 (boost) = 1.0
        assert_eq!(results[0].id, "old_popular");
        assert!(results[0].adjusted_score > results[1].adjusted_score);
    }
}
