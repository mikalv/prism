//! Ranking and relevance scoring module
//!
//! This module provides score adjustments for search results based on:
//! - Field boosting: weight certain fields higher than others
//! - Recency decay: boost newer documents over older ones
//! - Popularity boost: multiply scores by document-level boost values

pub mod collapse;
pub mod cross_encoder;
pub mod decay;
pub mod near_dup;
pub mod reranker;
pub mod score_function;

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use serde::Serialize;

pub use cross_encoder::CrossEncoderReranker;
pub use decay::{
    compute_decay, compute_decay_from_micros, parse_duration, DecayConfig, DecayFunction,
};
pub use reranker::{extract_text_from_result, RerankOptions, RerankRequest, Reranker};
pub use score_function::ScoreFunctionReranker;

use crate::schema::BoostingConfig;

/// How a score component contributes to the running total.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScoreOp {
    /// The starting point. Exactly one per explanation.
    Base { raw: f64 },
    /// Multiply the running score by `factor`.
    Multiply { factor: f64, result: f64 },
    /// Add `delta` to the running score.
    Add { delta: f64, result: f64 },
    /// Replace the running score with `value` (Phase 2 reranker).
    Replace { value: f64, previous: f64 },
}

/// One named step in the score pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreComponent {
    /// Human-readable stage name: "base", "recency_decay", "doc_boost",
    /// "signal:view_count", "rerank:cross_encoder", "rerank:score_function", …
    pub name: String,
    /// What this component did to the score.
    #[serde(flatten)]
    pub op: ScoreOp,
    /// Optional human note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Full breakdown for one result, evaluable top-to-bottom.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreExplanation {
    pub components: Vec<ScoreComponent>,
    /// The score after the last component applied. Equals SearchResult.score.
    pub final_score: f64,
}

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
            let function = r.decay_function.parse::<DecayFunction>().unwrap();
            let scale = parse_duration(&r.scale).unwrap_or(Duration::from_secs(7 * 86400));
            let offset = r.offset.as_ref().and_then(|s| parse_duration(s));

            let mut decay_config = DecayConfig::new(function, scale, r.decay_rate as f64);
            if let Some(offset) = offset {
                decay_config = decay_config.with_offset(offset);
            }
            decay_config
        });

        let signals = config
            .signals
            .iter()
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
    results: &mut [RankableResult],
    config: &RankingConfig,
    now: SystemTime,
    explain: bool,
) {
    for result in results.iter_mut() {
        let mut score = result.score as f64;
        let mut comps: Vec<ScoreComponent> = if explain {
            vec![ScoreComponent {
                name: "base".to_string(),
                op: ScoreOp::Base { raw: score },
                note: None,
            }]
        } else {
            Vec::new()
        };

        // Apply recency decay if configured
        if let Some(decay_config) = &config.recency_decay {
            if let Some(indexed_at_micros) = result.indexed_at_micros {
                let decay = compute_decay_from_micros(decay_config, indexed_at_micros, now);
                let new_score = score * decay;
                if explain {
                    comps.push(ScoreComponent {
                        name: "recency_decay".to_string(),
                        op: ScoreOp::Multiply {
                            factor: decay,
                            result: new_score,
                        },
                        note: Some(format!(
                            "{:?}, scale={}s",
                            decay_config.function,
                            decay_config.scale.as_secs()
                        )),
                    });
                }
                score = new_score;
            } else if explain {
                comps.push(ScoreComponent {
                    name: "recency_decay".to_string(),
                    op: ScoreOp::Multiply {
                        factor: 1.0,
                        result: score,
                    },
                    note: Some("no _indexed_at → skipped".to_string()),
                });
            }
        }

        // Apply document boost if present
        if let Some(boost) = result.boost {
            let new_score = score * boost;
            if explain {
                comps.push(ScoreComponent {
                    name: "doc_boost".to_string(),
                    op: ScoreOp::Multiply {
                        factor: boost,
                        result: new_score,
                    },
                    note: None,
                });
            }
            score = new_score;
        }

        // Apply custom ranking signals: each contributes field_value * weight
        for (field_name, weight) in &config.signals {
            if let Some(val) = result.fields.get(field_name) {
                let numeric = val
                    .as_f64()
                    .or_else(|| val.as_i64().map(|i| i as f64))
                    .or_else(|| val.as_u64().map(|u| u as f64));
                if let Some(v) = numeric {
                    let delta = v * (*weight as f64);
                    let new_score = score + delta;
                    if explain {
                        comps.push(ScoreComponent {
                            name: format!("signal:{}", field_name),
                            op: ScoreOp::Add {
                                delta,
                                result: new_score,
                            },
                            note: Some(format!("{}={} × weight={}", field_name, v, weight)),
                        });
                    }
                    score = new_score;
                }
            } else if explain {
                comps.push(ScoreComponent {
                    name: format!("signal:{}", field_name),
                    op: ScoreOp::Add {
                        delta: 0.0,
                        result: score,
                    },
                    note: Some("field missing → skipped".to_string()),
                });
            }
        }

        result.adjusted_score = score as f32;
        if explain {
            result.explanation = Some(ScoreExplanation {
                components: comps,
                final_score: score,
            });
        }
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
    /// Score breakdown (present only when explain was requested).
    pub explanation: Option<ScoreExplanation>,
}

impl RankableResult {
    /// Create from search result fields
    pub fn from_fields(id: String, score: f32, fields: HashMap<String, serde_json::Value>) -> Self {
        // Extract _indexed_at timestamp (stored as microseconds)
        let indexed_at_micros = fields.get("_indexed_at").and_then(|v| v.as_i64());

        // Extract _boost value
        let boost = fields.get("_boost").and_then(|v| v.as_f64());

        Self {
            id,
            score,
            adjusted_score: score, // Initially same as score
            fields,
            indexed_at_micros,
            boost,
            explanation: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(
        id: &str,
        score: f32,
        indexed_at_micros: Option<i64>,
        boost: Option<f64>,
    ) -> RankableResult {
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

        apply_ranking_adjustments(&mut results, &config, now, false);

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

        apply_ranking_adjustments(&mut results, &config, now, false);

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
            make_result(
                "old_popular",
                1.0,
                Some(now_micros - 7 * 86400 * 1_000_000),
                Some(3.0),
            ),
            make_result("new_regular", 1.0, Some(now_micros), Some(1.0)),
        ];

        apply_ranking_adjustments(&mut results, &config, now, false);

        // Old doc: 1.0 * 0.5 (decay) * 3.0 (boost) = 1.5
        // New doc: 1.0 * 1.0 (no decay) * 1.0 (boost) = 1.0
        assert_eq!(results[0].id, "old_popular");
        assert!(results[0].adjusted_score > results[1].adjusted_score);
    }

    // --- explain (score breakdown) tests ---

    fn make_result_with_signals(id: &str, score: f32, signals: &[(&str, f64)]) -> RankableResult {
        let mut fields = HashMap::new();
        for (name, val) in signals {
            fields.insert(name.to_string(), serde_json::json!(val));
        }
        RankableResult::from_fields(id.to_string(), score, fields)
    }

    #[test]
    fn explain_off_leaves_explanation_none() {
        let config = RankingConfig {
            field_weights: HashMap::new(),
            recency_decay: None,
            signals: vec![("views".to_string(), 0.01)],
        };
        let mut results = vec![make_result_with_signals("d1", 1.0, &[("views", 100.0)])];
        apply_ranking_adjustments(&mut results, &config, SystemTime::now(), false);
        assert!(
            results[0].explanation.is_none(),
            "explain=false must not populate explanation"
        );
    }

    #[test]
    fn explain_emits_base_component() {
        let config = RankingConfig {
            field_weights: HashMap::new(),
            recency_decay: None,
            signals: vec![],
        };
        let mut results = vec![make_result("d1", 5.0, None, None)];
        apply_ranking_adjustments(&mut results, &config, SystemTime::now(), true);

        let ex = results[0]
            .explanation
            .as_ref()
            .expect("explain=true must populate");
        assert_eq!(ex.components.len(), 1);
        assert_eq!(ex.components[0].name, "base");
        match ex.components[0].op {
            ScoreOp::Base { raw } => assert!((raw - 5.0).abs() < 1e-9),
            _ => panic!("expected Base op"),
        }
        assert!((ex.final_score - 5.0).abs() < 1e-6);
    }

    #[test]
    fn explain_records_recency_boost_and_skip() {
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
        let week_old = now_micros - (7 * 86400 * 1_000_000);

        let mut results = vec![
            make_result("recent", 1.0, Some(now_micros), None),
            make_result("stale", 1.0, Some(week_old), None),
            make_result("nots", 1.0, None, None), // no _indexed_at
        ];
        apply_ranking_adjustments(&mut results, &config, now, true);

        // recent: factor ~1.0
        let recent = results.iter().find(|r| r.id == "recent").unwrap();
        let rec_comp = recent
            .explanation
            .as_ref()
            .unwrap()
            .components
            .iter()
            .find(|c| c.name == "recency_decay")
            .unwrap();
        match rec_comp.op {
            ScoreOp::Multiply { factor, .. } => assert!(factor > 0.9),
            _ => panic!(),
        }

        // stale: factor ~0.5
        let stale = results.iter().find(|r| r.id == "stale").unwrap();
        let stale_comp = stale
            .explanation
            .as_ref()
            .unwrap()
            .components
            .iter()
            .find(|c| c.name == "recency_decay")
            .unwrap();
        match stale_comp.op {
            ScoreOp::Multiply { factor, .. } => assert!((factor - 0.5).abs() < 0.1),
            _ => panic!(),
        }

        // nots (no timestamp): skip note present
        let nots = results.iter().find(|r| r.id == "nots").unwrap();
        let nots_comp = nots
            .explanation
            .as_ref()
            .unwrap()
            .components
            .iter()
            .find(|c| c.name == "recency_decay")
            .unwrap();
        assert!(nots_comp.note.as_ref().unwrap().contains("skipped"));
    }

    #[test]
    fn explain_records_boost_and_signals_and_resums() {
        let config = RankingConfig {
            field_weights: HashMap::new(),
            recency_decay: None,
            signals: vec![("views".to_string(), 0.01)],
        };
        // score = 10.0, boost = 2.0, views = 300 → 10*2 + 300*0.01 = 23.0
        let mut results = vec![make_result_with_signals(
            "d1",
            10.0,
            &[("views", 300.0), ("_boost", 2.0)],
        )];
        apply_ranking_adjustments(&mut results, &config, SystemTime::now(), true);

        let ex = results[0].explanation.as_ref().unwrap();
        // Should have: base, doc_boost, signal:views
        assert_eq!(ex.components.len(), 3);
        assert_eq!(ex.components[0].name, "base");
        assert_eq!(ex.components[1].name, "doc_boost");
        assert_eq!(ex.components[2].name, "signal:views");

        // Replay the ops top-to-bottom and confirm we land on final_score.
        let mut running = 0.0f64;
        for c in &ex.components {
            running = match c.op {
                ScoreOp::Base { raw } => raw,
                ScoreOp::Multiply { result, .. } => result,
                ScoreOp::Add { result, .. } => result,
                ScoreOp::Replace { value, .. } => value,
            };
        }
        assert!(
            (running - ex.final_score).abs() < 1e-5,
            "replayed running={} should equal final_score={}",
            running,
            ex.final_score
        );
        assert!((ex.final_score - 23.0).abs() < 1e-4);
        assert!((results[0].adjusted_score as f64 - ex.final_score).abs() < 1e-4);
    }

    #[test]
    fn explain_missing_signal_field_is_noted() {
        let config = RankingConfig {
            field_weights: HashMap::new(),
            recency_decay: None,
            signals: vec![("missing".to_string(), 1.0)],
        };
        let mut results = vec![make_result("d1", 1.0, None, None)];
        apply_ranking_adjustments(&mut results, &config, SystemTime::now(), true);

        let sig = results[0]
            .explanation
            .as_ref()
            .unwrap()
            .components
            .iter()
            .find(|c| c.name == "signal:missing")
            .unwrap();
        assert!(sig.note.as_ref().unwrap().contains("skipped"));
    }
}
