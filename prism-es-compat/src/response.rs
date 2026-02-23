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
    #[serde(rename = "_source")]
    pub source: HashMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<HashMap<String, Vec<String>>>,
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
        let max_score = results.results.first().map(|r| r.score);

        let hits: Vec<Hit> = results
            .results
            .into_iter()
            .map(|r| Self::map_hit(index, r))
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
                total: TotalHits {
                    value: results.total,
                    relation: "eq".to_string(),
                },
                max_score,
                hits,
            },
            aggregations,
        }
    }

    fn map_hit(index: &str, result: SearchResult) -> Hit {
        Hit {
            index: index.to_string(),
            id: result.id,
            score: Some(result.score),
            source: result.fields,
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
                number: "7.17.0".to_string(), // Compatibility target
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

#[cfg(test)]
mod tests {
    use super::*;
    use prism::aggregations::{
        AggregationResult, AggregationValue, Bucket, PercentilesResult, StatsResult,
    };
    use prism::backends::{SearchResult, SearchResultsWithAggs};

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
        assert_eq!(info.version.number, "7.17.0");
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
            hit0.source.get("title"),
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
        hl.insert(
            "body".to_string(),
            vec!["<em>highlighted</em>".to_string()],
        );

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
                value: AggregationValue::Percentiles(PercentilesResult {
                    values: pct_values,
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
                assert_eq!(
                    buckets[0].key_as_string,
                    Some("active".to_string())
                );
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
                    key: "3.14".to_string(),
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
                // "3.14" can't parse as i64, should parse as f64
                match &buckets[0].key {
                    Value::Number(n) => {
                        assert!((n.as_f64().unwrap() - 3.14).abs() < 0.001);
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
                    source: {
                        let mut m = HashMap::new();
                        m.insert("title".to_string(), Value::String("doc".to_string()));
                        m
                    },
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
        let result = EsAggregationResult::Value { value: Some(3.14) };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("3.14"));
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
