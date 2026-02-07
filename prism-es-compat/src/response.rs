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
    Error {
        error: EsError,
        status: u16,
    },
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
