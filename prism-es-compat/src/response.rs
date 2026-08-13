//! Response mappers from Prism to Elasticsearch format

use prism::aggregations::{AggregationResult, AggregationValue, Bucket};
use prism::backends::{SearchResult, SearchResultsWithAggs};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// ES search response format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsSearchResponse {
    pub took: u64,
    pub timed_out: bool,
    #[serde(rename = "_shards")]
    pub shards: ShardStats,
    pub hits: HitsResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregations: Option<HashMap<String, EsAggregationResult>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardStats {
    pub total: u32,
    pub successful: u32,
    pub skipped: u32,
    pub failed: u32,
}

impl Default for ShardStats {
    fn default() -> Self {
        Self {
            total: 1,
            successful: 1,
            skipped: 0,
            failed: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitsResponse {
    pub total: TotalHits,
    pub max_score: Option<f32>,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotalHits {
    pub value: u64,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    #[serde(rename = "_index")]
    pub index: String,
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_score")]
    pub score: Option<f32>,
    /// ES sequence-number metadata. prism doesn't track real seq_no /
    /// primary_term, but the official ES client's taskManager and Discovery
    /// service validate these as integers on every hit; omitting them yields
    /// "_primary_term from elasticsearch must be an integer". Static defaults
    /// satisfy the validation (Kibana doesn't rely on the real values here).
    #[serde(rename = "_seq_no")]
    pub seq_no: i64,
    #[serde(rename = "_primary_term")]
    pub primary_term: i64,
    #[serde(rename = "_version")]
    pub version: i64,
    // None when the request set `_source: false`, so the key is omitted entirely.
    #[serde(rename = "_source", skip_serializing_if = "Option::is_none")]
    pub source: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<HashMap<String, Vec<String>>>,
}

/// Default number of hits counted exactly before `hits.total` becomes a lower
/// bound, matching Elasticsearch's `index.max_result_window`. The text backend
/// fetches up to this many + 1 documents, so a reported total greater than the
/// window means "there are at least this many".
pub const TRACK_TOTAL_HITS_WINDOW: u64 = 10_000;

/// Compute `hits.total` from the backend's (possibly window-capped) count and
/// the request's `track_total_hits`. Returns `relation: "eq"` when the count is
/// exact, or `"gte"` (with the value clamped to the tracking limit) when there
/// are more matching documents than were counted.
fn total_hits(backend_total: u64, track: Option<&crate::query::TrackTotalHits>) -> TotalHits {
    use crate::query::TrackTotalHits;
    let limit = match track {
        Some(TrackTotalHits::Count(n)) => (*n as u64).min(TRACK_TOTAL_HITS_WINDOW),
        // `true`/`false` fall back to the default window; exact tracking beyond
        // the window is not yet supported.
        _ => TRACK_TOTAL_HITS_WINDOW,
    };
    if backend_total > limit {
        TotalHits {
            value: limit,
            relation: "gte".to_string(),
        }
    } else {
        TotalHits {
            value: backend_total,
            relation: "eq".to_string(),
        }
    }
}

/// Match a field name against an ES `_source` pattern. Patterns support `*` as a
/// wildcard for any (possibly empty) sequence of characters (e.g. `*`, `user.*`,
/// `_*`, `*_id`); a pattern with no `*` matches exactly. Dotted names are treated
/// as literal characters, so `user.*` selects flat fields like `user.name`.
fn source_pattern_matches(pattern: &str, field: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == field;
    }
    // Anchored glob match with `*` = any run of characters.
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // Leading literal must be a prefix.
            if !field[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == parts.len() - 1 {
            // Trailing literal must be a suffix (at or after the current pos).
            return field[pos..].ends_with(part);
        } else if let Some(idx) = field[pos..].find(part) {
            pos += idx + part.len();
        } else {
            return false;
        }
    }
    true
}

fn matches_any(patterns: &[String], field: &str) -> bool {
    patterns.iter().any(|p| source_pattern_matches(p, field))
}

/// Flatten the `_dynamic` catch-all field into top-level fields.
///
/// Prism routes document fields not declared in the collection schema into a
/// stored `_dynamic` JSON object at index time (ES dynamic-mapping parity).
/// But ES clients (Kibana) expect those fields at the top level of `_source` —
/// e.g. a saved object's attributes live at `_source[doc.type]`, not under
/// `_dynamic`. Flattening here reconstructs the original document shape so
/// `_source` round-trips faithfully. Applied at both the search and GET
/// response choke points.
pub fn flatten_dynamic(mut fields: HashMap<String, Value>) -> HashMap<String, Value> {
    if let Some(dynamic) = fields.remove("_dynamic") {
        if let Value::Object(map) = dynamic {
            for (k, v) in map {
                fields.entry(k).or_insert(v);
            }
        }
    }
    fields
}

/// Apply an ES `_source` filter to a hit's fields. Returns `None` when the
/// source should be omitted entirely (`_source: false`). With no filter, all
/// fields are returned. Include/exclude patterns support `*` wildcards; names
/// with dots are matched as literal flat field names (Prism fields are flat).
fn apply_source_filter(
    fields: HashMap<String, Value>,
    source: Option<&crate::query::SourceFilter>,
) -> Option<HashMap<String, Value>> {
    use crate::query::SourceFilter;
    let fields = flatten_dynamic(fields);
    match source {
        None | Some(SourceFilter::Bool(true)) => Some(fields),
        Some(SourceFilter::Bool(false)) => None,
        Some(SourceFilter::Fields(includes)) => Some(
            fields
                .into_iter()
                .filter(|(k, _)| matches_any(includes, k))
                .collect(),
        ),
        Some(SourceFilter::Object { includes, excludes }) => {
            let mut out: HashMap<String, Value> = match includes {
                Some(inc) if !inc.is_empty() => fields
                    .into_iter()
                    .filter(|(k, _)| matches_any(inc, k))
                    .collect(),
                _ => fields,
            };
            if let Some(exc) = excludes {
                out.retain(|k, _| !matches_any(exc, k));
            }
            Some(out)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EsAggregationResult {
    Buckets {
        buckets: Vec<EsBucket>,
    },
    Value {
        value: Option<f64>,
    },
    Stats {
        count: u64,
        min: Option<f64>,
        max: Option<f64>,
        sum: Option<f64>,
        avg: Option<f64>,
    },
    Percentiles {
        values: HashMap<String, Option<f64>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsBucket {
    pub key: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_as_string: Option<String>,
    pub doc_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
    #[serde(flatten)]
    pub sub_aggs: HashMap<String, EsAggregationResult>,
}

/// Response mapper
pub struct ResponseMapper;

impl ResponseMapper {
    /// Convert Prism search results to ES format
    pub fn map_search_results(
        index: &str,
        results: SearchResultsWithAggs,
        took_ms: u64,
    ) -> EsSearchResponse {
        Self::map_search_results_with_source(index, results, took_ms, None, None)
    }

    /// Like [`map_search_results`], but applies an ES `_source` include/exclude
    /// filter to each hit's `_source` and computes `hits.total.relation` from the
    /// request's `track_total_hits`.
    pub fn map_search_results_with_source(
        index: &str,
        results: SearchResultsWithAggs,
        took_ms: u64,
        source: Option<&crate::query::SourceFilter>,
        track_total_hits: Option<&crate::query::TrackTotalHits>,
    ) -> EsSearchResponse {
        let max_score = results.results.first().map(|r| r.score);
        let total = total_hits(results.total, track_total_hits);

        let hits: Vec<Hit> = results
            .results
            .into_iter()
            .map(|r| Self::map_hit(index, r, source))
            .collect();

        let aggregations = if results.aggregations.is_empty() {
            None
        } else {
            Some(Self::map_aggregations(&results.aggregations))
        };

        EsSearchResponse {
            took: took_ms,
            timed_out: false,
            shards: ShardStats::default(),
            hits: HitsResponse {
                total,
                max_score,
                hits,
            },
            aggregations,
        }
    }

    fn map_hit(
        index: &str,
        result: SearchResult,
        source: Option<&crate::query::SourceFilter>,
    ) -> Hit {
        Hit {
            index: index.to_string(),
            id: result.id,
            score: Some(result.score),
            seq_no: 0,
            primary_term: 1,
            version: 1,
            source: apply_source_filter(result.fields, source),
            highlight: result.highlight,
        }
    }

    fn map_aggregations(
        aggs: &HashMap<String, AggregationResult>,
    ) -> HashMap<String, EsAggregationResult> {
        aggs.iter()
            .map(|(name, result)| (name.clone(), Self::map_aggregation_result(result)))
            .collect()
    }

    fn map_aggregation_result(result: &AggregationResult) -> EsAggregationResult {
        match &result.value {
            AggregationValue::Single(v) => EsAggregationResult::Value { value: Some(*v) },

            AggregationValue::Stats(stats) => EsAggregationResult::Stats {
                count: stats.count,
                min: stats.min,
                max: stats.max,
                sum: stats.sum,
                avg: stats.avg,
            },

            AggregationValue::Percentiles(p) => EsAggregationResult::Percentiles {
                values: p.values.clone(),
            },

            AggregationValue::Buckets(buckets) => EsAggregationResult::Buckets {
                buckets: buckets.iter().map(Self::map_bucket).collect(),
            },
        }
    }

    fn map_bucket(bucket: &Bucket) -> EsBucket {
        // Try to parse key as number for proper JSON representation
        let key = bucket
            .key
            .parse::<i64>()
            .map(Value::from)
            .or_else(|_| bucket.key.parse::<f64>().map(Value::from))
            .unwrap_or_else(|_| Value::String(bucket.key.clone()));

        let sub_aggs = bucket
            .sub_aggs
            .as_ref()
            .map(|aggs| {
                aggs.iter()
                    .map(|a| (a.name.clone(), Self::map_aggregation_result(a)))
                    .collect()
            })
            .unwrap_or_default();

        EsBucket {
            key,
            key_as_string: Some(bucket.key.clone()),
            doc_count: bucket.doc_count,
            from: bucket.from,
            to: bucket.to,
            sub_aggs,
        }
    }
}

/// ES multi-search response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsMSearchResponse {
    pub took: u64,
    pub responses: Vec<EsMSearchItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EsMSearchItem {
    Success(EsSearchResponse),
    Error { error: EsError, status: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub reason: String,
}

/// ES bulk response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsBulkResponse {
    pub took: u64,
    pub errors: bool,
    pub items: Vec<BulkItemResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkItemResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<BulkItemResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<BulkItemResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<BulkItemResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete: Option<BulkItemResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkItemResult {
    #[serde(rename = "_index")]
    pub index: String,
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_version")]
    pub version: u64,
    #[serde(rename = "_seq_no")]
    pub seq_no: i64,
    #[serde(rename = "_primary_term")]
    pub primary_term: i64,
    pub result: String,
    #[serde(rename = "_shards")]
    pub shards: ShardStats,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<EsError>,
}

/// ES cluster health response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsClusterHealth {
    pub cluster_name: String,
    pub status: String,
    pub timed_out: bool,
    pub number_of_nodes: u32,
    pub number_of_data_nodes: u32,
    pub active_primary_shards: u32,
    pub active_shards: u32,
    pub relocating_shards: u32,
    pub initializing_shards: u32,
    pub unassigned_shards: u32,
    pub delayed_unassigned_shards: u32,
    pub number_of_pending_tasks: u32,
    pub number_of_in_flight_fetch: u32,
    pub task_max_waiting_in_queue_millis: u64,
    pub active_shards_percent_as_number: f64,
}

impl Default for EsClusterHealth {
    fn default() -> Self {
        Self {
            cluster_name: "prism".to_string(),
            status: "green".to_string(),
            timed_out: false,
            number_of_nodes: 1,
            number_of_data_nodes: 1,
            active_primary_shards: 1,
            active_shards: 1,
            relocating_shards: 0,
            initializing_shards: 0,
            unassigned_shards: 0,
            delayed_unassigned_shards: 0,
            number_of_pending_tasks: 0,
            number_of_in_flight_fetch: 0,
            task_max_waiting_in_queue_millis: 0,
            active_shards_percent_as_number: 100.0,
        }
    }
}

/// ES root info response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsRootInfo {
    pub name: String,
    pub cluster_name: String,
    pub cluster_uuid: String,
    pub version: EsVersion,
    pub tagline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsVersion {
    pub number: String,
    pub build_flavor: String,
    pub build_type: String,
    pub build_hash: String,
    pub build_date: String,
    pub build_snapshot: bool,
    pub lucene_version: String,
    pub minimum_wire_compatibility_version: String,
    pub minimum_index_compatibility_version: String,
}

impl Default for EsRootInfo {
    fn default() -> Self {
        Self {
            name: "prism".to_string(),
            cluster_name: "prism".to_string(),
            cluster_uuid: "prism-es-compat".to_string(),
            version: EsVersion {
                number: "9.5.0".to_string(), // Compatibility target
                build_flavor: "default".to_string(),
                build_type: "prism".to_string(),
                build_hash: "unknown".to_string(),
                build_date: "2024-01-01T00:00:00.000000Z".to_string(),
                build_snapshot: false,
                lucene_version: "8.11.1".to_string(),
                minimum_wire_compatibility_version: "6.8.0".to_string(),
                minimum_index_compatibility_version: "6.0.0-beta1".to_string(),
            },
            tagline: "You Know, for Search (powered by Prism)".to_string(),
        }
    }
}

/// ES cat indices response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsCatIndex {
    pub health: String,
    pub status: String,
    pub index: String,
    pub uuid: String,
    pub pri: String,
    pub rep: String,
    #[serde(rename = "docs.count")]
    pub docs_count: String,
    #[serde(rename = "docs.deleted")]
    pub docs_deleted: String,
    #[serde(rename = "store.size")]
    pub store_size: String,
    #[serde(rename = "pri.store.size")]
    pub pri_store_size: String,
}

/// ES mapping response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsMappingResponse {
    #[serde(flatten)]
    pub indices: HashMap<String, EsIndexMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsIndexMapping {
    pub mappings: EsMappings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsMappings {
    pub properties: HashMap<String, EsFieldMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsFieldMapping {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<HashMap<String, EsFieldMapping>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// ES document get response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsGetResponse {
    #[serde(rename = "_index")]
    pub index: String,
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_version")]
    pub version: u64,
    #[serde(rename = "_seq_no")]
    pub seq_no: i64,
    #[serde(rename = "_primary_term")]
    pub primary_term: i64,
    pub found: bool,
    #[serde(rename = "_source", skip_serializing_if = "Option::is_none")]
    pub source: Option<HashMap<String, Value>>,
}

/// ES index document response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsIndexResponse {
    #[serde(rename = "_index")]
    pub index: String,
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_version")]
    pub version: u64,
    #[serde(rename = "_seq_no")]
    pub seq_no: i64,
    #[serde(rename = "_primary_term")]
    pub primary_term: i64,
    pub result: String,
    #[serde(rename = "_shards")]
    pub shards: ShardStats,
}

/// ES delete document response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsDeleteResponse {
    #[serde(rename = "_index")]
    pub index: String,
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_version")]
    pub version: u64,
    #[serde(rename = "_seq_no")]
    pub seq_no: i64,
    #[serde(rename = "_primary_term")]
    pub primary_term: i64,
    pub result: String,
    #[serde(rename = "_shards")]
    pub shards: ShardStats,
}

/// ES count response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsCountResponse {
    pub count: u64,
    #[serde(rename = "_shards")]
    pub shards: ShardStats,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::SourceFilter;
    use prism::aggregations::{
        AggregationResult, AggregationValue, Bucket, PercentilesResult, StatsResult,
    };
    use prism::backends::{SearchResult, SearchResultsWithAggs};

    use crate::query::TrackTotalHits;

    #[test]
    fn test_total_hits_exact_below_window() {
        let t = total_hits(42, None);
        assert_eq!(t.value, 42);
        assert_eq!(t.relation, "eq");
    }

    #[test]
    fn test_total_hits_capped_reports_gte() {
        // Backend fetched window+1, signalling there are more than the window.
        let t = total_hits(TRACK_TOTAL_HITS_WINDOW + 1, None);
        assert_eq!(t.value, TRACK_TOTAL_HITS_WINDOW);
        assert_eq!(t.relation, "gte");
    }

    #[test]
    fn test_total_hits_exactly_window_is_exact() {
        // Exactly at the window (not window+1) → known exact, relation "eq".
        let t = total_hits(TRACK_TOTAL_HITS_WINDOW, None);
        assert_eq!(t.value, TRACK_TOTAL_HITS_WINDOW);
        assert_eq!(t.relation, "eq");
    }

    #[test]
    fn test_total_hits_track_count_limits() {
        // track_total_hits: 100 → count is considered accurate only up to 100.
        let t = total_hits(500, Some(&TrackTotalHits::Count(100)));
        assert_eq!(t.value, 100);
        assert_eq!(t.relation, "gte");
        let t2 = total_hits(50, Some(&TrackTotalHits::Count(100)));
        assert_eq!(t2.value, 50);
        assert_eq!(t2.relation, "eq");
    }

    fn sample_fields() -> HashMap<String, Value> {
        let mut m = HashMap::new();
        m.insert("title".to_string(), Value::String("hi".to_string()));
        m.insert("body".to_string(), Value::String("text".to_string()));
        m.insert("ts".to_string(), Value::String("2026".to_string()));
        m
    }

    #[test]
    fn test_source_filter_none_returns_all() {
        let out = apply_source_filter(sample_fields(), None).unwrap();
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_source_filter_false_omits_source() {
        let out = apply_source_filter(sample_fields(), Some(&SourceFilter::Bool(false)));
        assert!(out.is_none());
    }

    #[test]
    fn test_source_filter_true_returns_all() {
        let out = apply_source_filter(sample_fields(), Some(&SourceFilter::Bool(true))).unwrap();
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_source_filter_includes_list() {
        let filter = SourceFilter::Fields(vec!["title".to_string(), "ts".to_string()]);
        let out = apply_source_filter(sample_fields(), Some(&filter)).unwrap();
        let mut keys: Vec<&str> = out.keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["title", "ts"]);
    }

    #[test]
    fn test_source_filter_object_includes_and_excludes() {
        // includes wins the base set, then excludes are removed.
        let filter = SourceFilter::Object {
            includes: Some(vec!["title".to_string(), "body".to_string()]),
            excludes: Some(vec!["body".to_string()]),
        };
        let out = apply_source_filter(sample_fields(), Some(&filter)).unwrap();
        let keys: Vec<&str> = out.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["title"]);
    }

    #[test]
    fn test_source_filter_object_excludes_only() {
        // No includes => start from all fields, remove excludes.
        let filter = SourceFilter::Object {
            includes: None,
            excludes: Some(vec!["ts".to_string()]),
        };
        let out = apply_source_filter(sample_fields(), Some(&filter)).unwrap();
        let mut keys: Vec<&str> = out.keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["body", "title"]);
    }

    fn dotted_fields() -> HashMap<String, Value> {
        let mut m = HashMap::new();
        m.insert("content".to_string(), Value::String("c".to_string()));
        m.insert("user.name".to_string(), Value::String("n".to_string()));
        m.insert("user.age".to_string(), Value::String("a".to_string()));
        m.insert("_indexed_at".to_string(), Value::String("t".to_string()));
        m
    }

    #[test]
    fn test_source_filter_wildcard_star_includes_all() {
        let filter = SourceFilter::Fields(vec!["*".to_string()]);
        let out = apply_source_filter(dotted_fields(), Some(&filter)).unwrap();
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn test_source_filter_prefix_wildcard() {
        // "user.*" selects the dotted sub-fields, not `content`.
        let filter = SourceFilter::Fields(vec!["user.*".to_string()]);
        let out = apply_source_filter(dotted_fields(), Some(&filter)).unwrap();
        let mut keys: Vec<&str> = out.keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["user.age", "user.name"]);
    }

    #[test]
    fn test_source_filter_exclude_wildcard() {
        // Drop internal fields with a leading-underscore pattern.
        let filter = SourceFilter::Object {
            includes: None,
            excludes: Some(vec!["_*".to_string()]),
        };
        let out = apply_source_filter(dotted_fields(), Some(&filter)).unwrap();
        assert!(!out.contains_key("_indexed_at"));
        assert!(out.contains_key("content"));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn test_source_filter_include_and_exclude_wildcards() {
        // Include everything, then drop the user.* sub-fields.
        let filter = SourceFilter::Object {
            includes: Some(vec!["*".to_string()]),
            excludes: Some(vec!["user.*".to_string()]),
        };
        let out = apply_source_filter(dotted_fields(), Some(&filter)).unwrap();
        let mut keys: Vec<&str> = out.keys().map(|s| s.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["_indexed_at", "content"]);
    }

    #[test]
    fn test_source_filter_exact_still_works() {
        // A literal name with no wildcard still matches exactly.
        let filter = SourceFilter::Fields(vec!["content".to_string()]);
        let out = apply_source_filter(dotted_fields(), Some(&filter)).unwrap();
        let keys: Vec<&str> = out.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["content"]);
    }

    #[test]
    fn test_source_pattern_matches_variants() {
        assert!(source_pattern_matches("*", "anything"));
        assert!(source_pattern_matches("user.*", "user.name"));
        assert!(!source_pattern_matches("user.*", "content"));
        assert!(source_pattern_matches("*_id", "session_id"));
        assert!(!source_pattern_matches("*_id", "session"));
        assert!(source_pattern_matches("a*c", "abc"));
        assert!(source_pattern_matches("a*c", "ac"));
        assert!(!source_pattern_matches("a*c", "ab"));
        // No wildcard → exact match.
        assert!(source_pattern_matches("role", "role"));
        assert!(!source_pattern_matches("rol", "role"));
    }

    // ===================================================================
    // ShardStats default
    // ===================================================================

    #[test]
    fn test_shard_stats_default() {
        let s = ShardStats::default();
        assert_eq!(s.total, 1);
        assert_eq!(s.successful, 1);
        assert_eq!(s.skipped, 0);
        assert_eq!(s.failed, 0);
    }

    // ===================================================================
    // EsClusterHealth default
    // ===================================================================

    #[test]
    fn test_cluster_health_default() {
        let h = EsClusterHealth::default();
        assert_eq!(h.cluster_name, "prism");
        assert_eq!(h.status, "green");
        assert!(!h.timed_out);
        assert_eq!(h.number_of_nodes, 1);
        assert_eq!(h.number_of_data_nodes, 1);
        assert_eq!(h.active_primary_shards, 1);
        assert_eq!(h.active_shards, 1);
        assert_eq!(h.active_shards_percent_as_number, 100.0);
    }

    // ===================================================================
    // EsRootInfo default
    // ===================================================================

    #[test]
    fn test_root_info_default() {
        let info = EsRootInfo::default();
        assert_eq!(info.name, "prism");
        assert_eq!(info.cluster_name, "prism");
        assert_eq!(info.version.number, "9.5.0");
        assert_eq!(info.version.build_type, "prism");
        assert!(!info.version.build_snapshot);
        assert!(info.tagline.contains("Prism"));
    }

    // ===================================================================
    // ResponseMapper::map_search_results — empty results
    // ===================================================================

    #[test]
    fn test_map_search_results_empty() {
        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: HashMap::new(),
        };
        let response = ResponseMapper::map_search_results("test_index", results, 5);
        assert_eq!(response.took, 5);
        assert!(!response.timed_out);
        assert_eq!(response.hits.total.value, 0);
        assert_eq!(response.hits.total.relation, "eq");
        assert!(response.hits.max_score.is_none());
        assert!(response.hits.hits.is_empty());
        assert!(response.aggregations.is_none());
        assert_eq!(response.shards.total, 1);
        assert_eq!(response.shards.successful, 1);
    }

    // ===================================================================
    // ResponseMapper::map_search_results — with hits
    // ===================================================================

    #[test]
    fn test_map_search_results_with_hits() {
        let mut fields1 = HashMap::new();
        fields1.insert("title".to_string(), Value::String("doc1".to_string()));
        let mut fields2 = HashMap::new();
        fields2.insert("title".to_string(), Value::String("doc2".to_string()));

        let results = SearchResultsWithAggs {
            results: vec![
                SearchResult {
                    id: "id1".to_string(),
                    score: 1.5,
                    fields: fields1,
                    highlight: None,
                },
                SearchResult {
                    id: "id2".to_string(),
                    score: 0.8,
                    fields: fields2,
                    highlight: None,
                },
            ],
            total: 2,
            aggregations: HashMap::new(),
        };
        let response = ResponseMapper::map_search_results("my_index", results, 10);

        assert_eq!(response.hits.total.value, 2);
        assert_eq!(response.hits.max_score, Some(1.5));
        assert_eq!(response.hits.hits.len(), 2);

        let hit0 = &response.hits.hits[0];
        assert_eq!(hit0.index, "my_index");
        assert_eq!(hit0.id, "id1");
        assert_eq!(hit0.score, Some(1.5));
        assert_eq!(
            hit0.source.as_ref().unwrap().get("title"),
            Some(&Value::String("doc1".to_string()))
        );
        assert!(hit0.highlight.is_none());

        let hit1 = &response.hits.hits[1];
        assert_eq!(hit1.id, "id2");
        assert_eq!(hit1.score, Some(0.8));
    }

    // ===================================================================
    // ResponseMapper — with highlights
    // ===================================================================

    #[test]
    fn test_map_search_results_with_highlight() {
        let mut fields = HashMap::new();
        fields.insert("body".to_string(), Value::String("text".to_string()));

        let mut hl = HashMap::new();
        hl.insert("body".to_string(), vec!["<em>highlighted</em>".to_string()]);

        let results = SearchResultsWithAggs {
            results: vec![SearchResult {
                id: "h1".to_string(),
                score: 2.0,
                fields,
                highlight: Some(hl),
            }],
            total: 1,
            aggregations: HashMap::new(),
        };

        let response = ResponseMapper::map_search_results("idx", results, 1);
        let hit = &response.hits.hits[0];
        let highlight = hit.highlight.as_ref().unwrap();
        assert_eq!(
            highlight.get("body").unwrap(),
            &vec!["<em>highlighted</em>".to_string()]
        );
    }

    // ===================================================================
    // Aggregation result mapping — Single value
    // ===================================================================

    #[test]
    fn test_map_agg_single_value() {
        let mut aggs = HashMap::new();
        aggs.insert(
            "avg_price".to_string(),
            AggregationResult {
                name: "avg_price".to_string(),
                value: AggregationValue::Single(42.5),
            },
        );

        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: aggs,
        };
        let response = ResponseMapper::map_search_results("idx", results, 1);
        let es_aggs = response.aggregations.unwrap();
        match &es_aggs["avg_price"] {
            EsAggregationResult::Value { value } => {
                assert_eq!(*value, Some(42.5));
            }
            _ => panic!("Expected Value aggregation result"),
        }
    }

    // ===================================================================
    // Aggregation result mapping — Stats
    // ===================================================================

    #[test]
    fn test_map_agg_stats() {
        let mut aggs = HashMap::new();
        aggs.insert(
            "rt_stats".to_string(),
            AggregationResult {
                name: "rt_stats".to_string(),
                value: AggregationValue::Stats(StatsResult {
                    count: 100,
                    min: Some(1.0),
                    max: Some(500.0),
                    sum: Some(10000.0),
                    avg: Some(100.0),
                }),
            },
        );

        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: aggs,
        };
        let response = ResponseMapper::map_search_results("idx", results, 1);
        let es_aggs = response.aggregations.unwrap();
        match &es_aggs["rt_stats"] {
            EsAggregationResult::Stats {
                count,
                min,
                max,
                sum,
                avg,
            } => {
                assert_eq!(*count, 100);
                assert_eq!(*min, Some(1.0));
                assert_eq!(*max, Some(500.0));
                assert_eq!(*sum, Some(10000.0));
                assert_eq!(*avg, Some(100.0));
            }
            _ => panic!("Expected Stats aggregation result"),
        }
    }

    // ===================================================================
    // Aggregation result mapping — Percentiles
    // ===================================================================

    #[test]
    fn test_map_agg_percentiles() {
        let mut pct_values = HashMap::new();
        pct_values.insert("50.0".to_string(), Some(12.5));
        pct_values.insert("95.0".to_string(), Some(42.0));

        let mut aggs = HashMap::new();
        aggs.insert(
            "lat_pct".to_string(),
            AggregationResult {
                name: "lat_pct".to_string(),
                value: AggregationValue::Percentiles(PercentilesResult { values: pct_values }),
            },
        );

        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: aggs,
        };
        let response = ResponseMapper::map_search_results("idx", results, 1);
        let es_aggs = response.aggregations.unwrap();
        match &es_aggs["lat_pct"] {
            EsAggregationResult::Percentiles { values } => {
                assert_eq!(*values.get("50.0").unwrap(), Some(12.5));
                assert_eq!(*values.get("95.0").unwrap(), Some(42.0));
            }
            _ => panic!("Expected Percentiles aggregation result"),
        }
    }

    // ===================================================================
    // Aggregation result mapping — Buckets
    // ===================================================================

    #[test]
    fn test_map_agg_buckets_string_key() {
        let mut aggs = HashMap::new();
        aggs.insert(
            "by_status".to_string(),
            AggregationResult {
                name: "by_status".to_string(),
                value: AggregationValue::Buckets(vec![
                    Bucket {
                        key: "active".to_string(),
                        doc_count: 42,
                        from: None,
                        to: None,
                        sub_aggs: None,
                    },
                    Bucket {
                        key: "inactive".to_string(),
                        doc_count: 8,
                        from: None,
                        to: None,
                        sub_aggs: None,
                    },
                ]),
            },
        );

        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: aggs,
        };
        let response = ResponseMapper::map_search_results("idx", results, 1);
        let es_aggs = response.aggregations.unwrap();
        match &es_aggs["by_status"] {
            EsAggregationResult::Buckets { buckets } => {
                assert_eq!(buckets.len(), 2);
                assert_eq!(buckets[0].key, Value::String("active".to_string()));
                assert_eq!(buckets[0].doc_count, 42);
                assert_eq!(buckets[0].key_as_string, Some("active".to_string()));
                assert!(buckets[0].sub_aggs.is_empty());
            }
            _ => panic!("Expected Buckets aggregation result"),
        }
    }

    #[test]
    fn test_map_agg_buckets_numeric_key() {
        let mut aggs = HashMap::new();
        aggs.insert(
            "by_code".to_string(),
            AggregationResult {
                name: "by_code".to_string(),
                value: AggregationValue::Buckets(vec![Bucket {
                    key: "200".to_string(),
                    doc_count: 100,
                    from: None,
                    to: None,
                    sub_aggs: None,
                }]),
            },
        );

        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: aggs,
        };
        let response = ResponseMapper::map_search_results("idx", results, 1);
        let es_aggs = response.aggregations.unwrap();
        match &es_aggs["by_code"] {
            EsAggregationResult::Buckets { buckets } => {
                // "200" should parse as i64
                assert_eq!(buckets[0].key, Value::Number(serde_json::Number::from(200)));
                assert_eq!(buckets[0].key_as_string, Some("200".to_string()));
            }
            _ => panic!("Expected Buckets"),
        }
    }

    #[test]
    fn test_map_agg_buckets_float_key() {
        let mut aggs = HashMap::new();
        aggs.insert(
            "by_score".to_string(),
            AggregationResult {
                name: "by_score".to_string(),
                value: AggregationValue::Buckets(vec![Bucket {
                    key: "2.78".to_string(),
                    doc_count: 5,
                    from: None,
                    to: None,
                    sub_aggs: None,
                }]),
            },
        );

        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: aggs,
        };
        let response = ResponseMapper::map_search_results("idx", results, 1);
        let es_aggs = response.aggregations.unwrap();
        match &es_aggs["by_score"] {
            EsAggregationResult::Buckets { buckets } => {
                // "2.78" can't parse as i64, should parse as f64
                match &buckets[0].key {
                    Value::Number(n) => {
                        assert!((n.as_f64().unwrap() - 2.78).abs() < 0.01);
                    }
                    _ => panic!("Expected numeric key"),
                }
            }
            _ => panic!("Expected Buckets"),
        }
    }

    #[test]
    fn test_map_agg_buckets_with_from_to() {
        let mut aggs = HashMap::new();
        aggs.insert(
            "price_ranges".to_string(),
            AggregationResult {
                name: "price_ranges".to_string(),
                value: AggregationValue::Buckets(vec![Bucket {
                    key: "cheap".to_string(),
                    doc_count: 10,
                    from: None,
                    to: Some(50.0),
                    sub_aggs: None,
                }]),
            },
        );

        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: aggs,
        };
        let response = ResponseMapper::map_search_results("idx", results, 1);
        let es_aggs = response.aggregations.unwrap();
        match &es_aggs["price_ranges"] {
            EsAggregationResult::Buckets { buckets } => {
                assert!(buckets[0].from.is_none());
                assert_eq!(buckets[0].to, Some(50.0));
            }
            _ => panic!("Expected Buckets"),
        }
    }

    #[test]
    fn test_map_agg_buckets_with_sub_aggs() {
        let sub = vec![AggregationResult {
            name: "avg_price".to_string(),
            value: AggregationValue::Single(25.0),
        }];

        let mut aggs = HashMap::new();
        aggs.insert(
            "by_cat".to_string(),
            AggregationResult {
                name: "by_cat".to_string(),
                value: AggregationValue::Buckets(vec![Bucket {
                    key: "electronics".to_string(),
                    doc_count: 50,
                    from: None,
                    to: None,
                    sub_aggs: Some(sub),
                }]),
            },
        );

        let results = SearchResultsWithAggs {
            results: vec![],
            total: 0,
            aggregations: aggs,
        };
        let response = ResponseMapper::map_search_results("idx", results, 1);
        let es_aggs = response.aggregations.unwrap();
        match &es_aggs["by_cat"] {
            EsAggregationResult::Buckets { buckets } => {
                let sub = &buckets[0].sub_aggs;
                assert!(sub.contains_key("avg_price"));
                match &sub["avg_price"] {
                    EsAggregationResult::Value { value } => {
                        assert_eq!(*value, Some(25.0));
                    }
                    _ => panic!("Expected Value sub-aggregation"),
                }
            }
            _ => panic!("Expected Buckets"),
        }
    }

    // ===================================================================
    // Serde round-trip tests for response types
    // ===================================================================

    #[test]
    fn test_search_response_serde_roundtrip() {
        let response = EsSearchResponse {
            took: 15,
            timed_out: false,
            shards: ShardStats::default(),
            hits: HitsResponse {
                total: TotalHits {
                    value: 1,
                    relation: "eq".to_string(),
                },
                max_score: Some(1.0),
                hits: vec![Hit {
                    index: "test".to_string(),
                    id: "1".to_string(),
                    score: Some(1.0),
                    source: Some({
                        let mut m = HashMap::new();
                        m.insert("title".to_string(), Value::String("doc".to_string()));
                        m
                    }),
                    highlight: None,
                }],
            },
            aggregations: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        let deser: EsSearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.took, 15);
        assert_eq!(deser.hits.total.value, 1);
        assert_eq!(deser.hits.hits[0].id, "1");
    }

    #[test]
    fn test_bulk_response_serde() {
        let response = EsBulkResponse {
            took: 5,
            errors: false,
            items: vec![BulkItemResponse {
                index: Some(BulkItemResult {
                    index: "test".to_string(),
                    id: "1".to_string(),
                    version: 1,
                    result: "created".to_string(),
                    shards: ShardStats::default(),
                    status: 201,
                    error: None,
                }),
                create: None,
                delete: None,
            }],
        };

        let json = serde_json::to_string(&response).unwrap();
        let deser: EsBulkResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.took, 5);
        assert!(!deser.errors);
        assert_eq!(deser.items.len(), 1);
    }

    #[test]
    fn test_cat_index_serde() {
        let idx = EsCatIndex {
            health: "green".to_string(),
            status: "open".to_string(),
            index: "logs".to_string(),
            uuid: "abc123".to_string(),
            pri: "1".to_string(),
            rep: "0".to_string(),
            docs_count: "1000".to_string(),
            docs_deleted: "0".to_string(),
            store_size: "1.2mb".to_string(),
            pri_store_size: "1.2mb".to_string(),
        };
        let json = serde_json::to_string(&idx).unwrap();
        assert!(json.contains("docs.count"));
        assert!(json.contains("store.size"));
        let deser: EsCatIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.index, "logs");
        assert_eq!(deser.docs_count, "1000");
    }

    #[test]
    fn test_mapping_response_serde() {
        let mut properties = HashMap::new();
        properties.insert(
            "title".to_string(),
            EsFieldMapping {
                field_type: "text".to_string(),
                fields: None,
                format: None,
            },
        );
        let mut indices = HashMap::new();
        indices.insert(
            "my_index".to_string(),
            EsIndexMapping {
                mappings: EsMappings { properties },
            },
        );

        let resp = EsMappingResponse { indices };
        let json = serde_json::to_string(&resp).unwrap();
        let deser: EsMappingResponse = serde_json::from_str(&json).unwrap();
        assert!(deser.indices.contains_key("my_index"));
    }

    #[test]
    fn test_msearch_response_serde() {
        let response = EsMSearchResponse {
            took: 10,
            responses: vec![
                EsMSearchItem::Success(EsSearchResponse {
                    took: 5,
                    timed_out: false,
                    shards: ShardStats::default(),
                    hits: HitsResponse {
                        total: TotalHits {
                            value: 0,
                            relation: "eq".to_string(),
                        },
                        max_score: None,
                        hits: vec![],
                    },
                    aggregations: None,
                }),
                EsMSearchItem::Error {
                    error: EsError {
                        error_type: "index_not_found_exception".to_string(),
                        reason: "no such index [missing]".to_string(),
                    },
                    status: 404,
                },
            ],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("index_not_found_exception"));
    }

    #[test]
    fn test_aggregation_result_serde_value() {
        let result = EsAggregationResult::Value { value: Some(2.78) };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("2.78"));
    }

    #[test]
    fn test_aggregation_result_serde_stats() {
        let result = EsAggregationResult::Stats {
            count: 10,
            min: Some(1.0),
            max: Some(100.0),
            sum: Some(500.0),
            avg: Some(50.0),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"count\":10"));
    }

    #[test]
    fn test_es_error_serde() {
        let error = EsError {
            error_type: "test_error".to_string(),
            reason: "something broke".to_string(),
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("test_error"));
        assert!(json.contains("something broke"));
    }
}
