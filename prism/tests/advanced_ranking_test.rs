//! Tests for advanced ranking features (#56)
//!
//! Tests: HybridConfig wiring, per-query rrf_k, min_score, score_function,
//! normalization modes, BM25 lint warnings.

use prism::backends::r#trait::{SearchResult, SearchResults};
use prism::backends::HybridSearchCoordinator;
use prism::ranking::score_function::ScoreFunctionReranker;
use prism::schema::loader::SchemaLoader;
use prism::schema::types::{
    CollectionSchema, HybridConfig, ScoreNormalization, VectorDistance,
};
use serde_json::json;
use std::collections::HashMap;

// ── Helper ──────────────────────────────────────────────────────────────────

fn make_results(items: Vec<(&str, f32)>) -> SearchResults {
    let results: Vec<SearchResult> = items
        .into_iter()
        .map(|(id, score)| SearchResult {
            id: id.to_string(),
            score,
            fields: HashMap::new(),
            highlight: None,
        })
        .collect();
    let total = results.len();
    SearchResults { results, total, latency_ms: 0 }
}

fn make_results_with_fields(
    items: Vec<(&str, f32, HashMap<String, serde_json::Value>)>,
) -> SearchResults {
    let results: Vec<SearchResult> = items
        .into_iter()
        .map(|(id, score, fields)| SearchResult {
            id: id.to_string(),
            score,
            fields,
            highlight: None,
        })
        .collect();
    let total = results.len();
    SearchResults { results, total, latency_ms: 0 }
}

// ── Normalization: MaxNorm ──────────────────────────────────────────────────

#[test]
fn test_merge_weighted_maxnorm() {
    let text = make_results(vec![("a", 10.0), ("b", 5.0)]);
    let vector = make_results(vec![("a", 0.9), ("c", 0.3)]);

    let merged = HybridSearchCoordinator::merge_weighted_with_normalization(
        text,
        vector,
        0.5, // text_weight
        0.5, // vector_weight
        10,
        &ScoreNormalization::MaxNorm,
        None,
    );

    // text: a=10/10=1.0, b=5/10=0.5 (normalized by max=10)
    // vector: a=0.9/0.9=1.0, c=0.3/0.9≈0.333 (normalized by max=0.9)
    // combined: a = 0.5*1.0 + 0.5*1.0 = 1.0
    //           b = 0.5*0.5 = 0.25
    //           c = 0.5*0.333 ≈ 0.167
    assert_eq!(merged.results.len(), 3);
    let a = merged.results.iter().find(|r| r.id == "a").unwrap();
    assert!((a.score - 1.0).abs() < 0.01, "a.score = {}", a.score);
}

// ── Normalization: None ─────────────────────────────────────────────────────

#[test]
fn test_merge_weighted_no_normalization() {
    let text = make_results(vec![("a", 10.0), ("b", 5.0)]);
    let vector = make_results(vec![("a", 0.9)]);

    let merged = HybridSearchCoordinator::merge_weighted_with_normalization(
        text,
        vector,
        0.5,
        0.5,
        10,
        &ScoreNormalization::None,
        None,
    );

    // No normalization: raw scores
    // a = 0.5*10.0 + 0.5*0.9 = 5.45
    // b = 0.5*5.0 = 2.5
    let a = merged.results.iter().find(|r| r.id == "a").unwrap();
    assert!((a.score - 5.45).abs() < 0.01, "a.score = {}", a.score);

    let b = merged.results.iter().find(|r| r.id == "b").unwrap();
    assert!((b.score - 2.5).abs() < 0.01, "b.score = {}", b.score);
}

// ── Normalization: MetricAware (Cosine) ─────────────────────────────────────

#[test]
fn test_merge_metric_aware_cosine() {
    let text = make_results(vec![("a", 10.0)]);
    let vector = make_results(vec![("a", 0.8)]);

    let merged = HybridSearchCoordinator::merge_weighted_with_normalization(
        text,
        vector,
        0.5,
        0.5,
        10,
        &ScoreNormalization::MetricAware,
        Some(&VectorDistance::Cosine),
    );

    // text: a=10/10=1.0 (BM25 always normalized by max)
    // vector: cosine scores used as-is → 0.8
    // a = 0.5*1.0 + 0.5*0.8 = 0.9
    let a = merged.results.iter().find(|r| r.id == "a").unwrap();
    assert!((a.score - 0.9).abs() < 0.01, "a.score = {}", a.score);
}

// ── Normalization: MetricAware (Euclidean) ──────────────────────────────────

#[test]
fn test_merge_metric_aware_euclidean() {
    let text = make_results(vec![("a", 6.0)]);
    // Euclidean: 1-dist can go negative; clamped then divided by max
    let vector = make_results(vec![("a", -0.5), ("b", 0.8)]);

    let merged = HybridSearchCoordinator::merge_weighted_with_normalization(
        text,
        vector,
        0.5,
        0.5,
        10,
        &ScoreNormalization::MetricAware,
        Some(&VectorDistance::Euclidean),
    );

    // text: a=6/6=1.0
    // vector: a = max(0, -0.5)=0 / 0.8 = 0; b = 0.8/0.8=1.0
    // a = 0.5*1.0 + 0.5*0.0 = 0.5
    // b = 0.5*1.0 = 0.5
    let a = merged.results.iter().find(|r| r.id == "a").unwrap();
    assert!((a.score - 0.5).abs() < 0.01, "a.score = {}", a.score);
}

// ── Normalization: MetricAware (Dot) ────────────────────────────────────────

#[test]
fn test_merge_metric_aware_dot() {
    let text = make_results(vec![("a", 4.0)]);
    let vector = make_results(vec![("a", 20.0), ("b", 10.0)]);

    let merged = HybridSearchCoordinator::merge_weighted_with_normalization(
        text,
        vector,
        0.5,
        0.5,
        10,
        &ScoreNormalization::MetricAware,
        Some(&VectorDistance::Dot),
    );

    // text: a=4/4=1.0
    // vector (dot): a=20/20=1.0, b=10/20=0.5
    // a = 0.5*1.0 + 0.5*1.0 = 1.0
    // b = 0.5*0.5 = 0.25
    let a = merged.results.iter().find(|r| r.id == "a").unwrap();
    assert!((a.score - 1.0).abs() < 0.01, "a.score = {}", a.score);
}

// ── Score function: basic expression ────────────────────────────────────────

#[test]
fn test_score_function_doubles_scores() {
    let reranker = ScoreFunctionReranker::new("_score * 2").unwrap();
    let score = reranker.evaluate(3.0, &HashMap::new());
    assert!((score - 6.0).abs() < 0.001);
}

#[test]
fn test_score_function_with_field() {
    let reranker = ScoreFunctionReranker::new("_score + log(likes + 1)").unwrap();
    let fields = HashMap::from([("likes".to_string(), json!(99))]);
    // 1.0 + ln(100) ≈ 5.605
    let score = reranker.evaluate(1.0, &fields);
    assert!((score - 5.605).abs() < 0.05);
}

// ── Min score filtering ─────────────────────────────────────────────────────

#[test]
fn test_min_score_filter() {
    let mut results = make_results(vec![("a", 0.9), ("b", 0.3), ("c", 0.7), ("d", 0.1)]);

    let min = 0.5_f32;
    results.results.retain(|r| r.score >= min);
    results.total = results.results.len();

    assert_eq!(results.total, 2);
    assert!(results.results.iter().all(|r| r.score >= 0.5));
}

// ── HybridConfig schema defaults ────────────────────────────────────────────

#[test]
fn test_hybrid_config_defaults() {
    let config = HybridConfig::default();
    assert_eq!(config.default_strategy, "rrf");
    assert_eq!(config.rrf_k, 60);
    assert_eq!(config.text_weight, 0.5);
    assert_eq!(config.vector_weight, 0.5);
    assert_eq!(config.normalization, ScoreNormalization::MaxNorm);
}

#[test]
fn test_hybrid_config_deserialization() {
    let yaml = r#"
default_strategy: weighted
rrf_k: 100
text_weight: 0.7
vector_weight: 0.3
normalization: metric_aware
"#;
    let config: HybridConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.default_strategy, "weighted");
    assert_eq!(config.rrf_k, 100);
    assert!((config.text_weight - 0.7).abs() < 0.001);
    assert!((config.vector_weight - 0.3).abs() < 0.001);
    assert_eq!(config.normalization, ScoreNormalization::MetricAware);
}

#[test]
fn test_score_normalization_deserialization() {
    let none: ScoreNormalization = serde_yaml::from_str("none").unwrap();
    assert_eq!(none, ScoreNormalization::None);

    let max: ScoreNormalization = serde_yaml::from_str("max_norm").unwrap();
    assert_eq!(max, ScoreNormalization::MaxNorm);

    let metric: ScoreNormalization = serde_yaml::from_str("metric_aware").unwrap();
    assert_eq!(metric, ScoreNormalization::MetricAware);
}

// ── BM25 lint warnings ─────────────────────────────────────────────────────

#[test]
fn test_bm25_lint_warning_nondefault() {
    let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
    bm25_k1: 1.5
    bm25_b: 0.6
"#;
    let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
    let issues = SchemaLoader::lint_schema(&schema);

    assert!(
        issues.iter().any(|i| i.contains("bm25_k1") && i.contains("1.5")),
        "Should warn about non-default bm25_k1; got: {:?}",
        issues
    );
    assert!(
        issues.iter().any(|i| i.contains("bm25_b") && i.contains("0.6")),
        "Should warn about non-default bm25_b; got: {:?}",
        issues
    );
}

#[test]
fn test_bm25_lint_no_warning_defaults() {
    let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
    bm25_k1: 1.2
    bm25_b: 0.75
"#;
    let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
    let issues = SchemaLoader::lint_schema(&schema);

    // Default values should NOT produce warnings
    assert!(
        !issues.iter().any(|i| i.contains("bm25_k1")),
        "Should NOT warn about default bm25_k1; got: {:?}",
        issues
    );
    assert!(
        !issues.iter().any(|i| i.contains("bm25_b")),
        "Should NOT warn about default bm25_b; got: {:?}",
        issues
    );
}

#[test]
fn test_bm25_lint_no_warning_when_unset() {
    let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
"#;
    let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
    let issues = SchemaLoader::lint_schema(&schema);

    assert!(
        !issues.iter().any(|i| i.contains("bm25")),
        "Should NOT warn when bm25 params are unset; got: {:?}",
        issues
    );
}

// ── Score function post-search simulation ───────────────────────────────────

#[test]
fn test_score_function_reranks_and_sorts() {
    let reranker = ScoreFunctionReranker::new("_score * popularity").unwrap();

    let mut results = make_results_with_fields(vec![
        (
            "a",
            1.0,
            HashMap::from([("popularity".to_string(), json!(10))]),
        ),
        (
            "b",
            2.0,
            HashMap::from([("popularity".to_string(), json!(1))]),
        ),
        (
            "c",
            0.5,
            HashMap::from([("popularity".to_string(), json!(100))]),
        ),
    ]);

    // Apply score function
    for result in results.results.iter_mut() {
        result.score = reranker.evaluate(result.score, &result.fields);
    }
    results
        .results
        .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

    // c: 0.5*100=50, a: 1.0*10=10, b: 2.0*1=2
    assert_eq!(results.results[0].id, "c");
    assert_eq!(results.results[1].id, "a");
    assert_eq!(results.results[2].id, "b");
    assert!((results.results[0].score - 50.0).abs() < 0.01);
}

// ── Empty result sets ───────────────────────────────────────────────────────

#[test]
fn test_merge_empty_text_results() {
    let text = make_results(vec![]);
    let vector = make_results(vec![("a", 0.9)]);

    let merged = HybridSearchCoordinator::merge_weighted_with_normalization(
        text,
        vector,
        0.5,
        0.5,
        10,
        &ScoreNormalization::MaxNorm,
        None,
    );

    assert_eq!(merged.results.len(), 1);
    assert_eq!(merged.results[0].id, "a");
}

#[test]
fn test_merge_empty_vector_results() {
    let text = make_results(vec![("a", 5.0)]);
    let vector = make_results(vec![]);

    let merged = HybridSearchCoordinator::merge_weighted_with_normalization(
        text,
        vector,
        0.5,
        0.5,
        10,
        &ScoreNormalization::MaxNorm,
        None,
    );

    assert_eq!(merged.results.len(), 1);
    assert_eq!(merged.results[0].id, "a");
}
