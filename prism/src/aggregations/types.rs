use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationRequest {
    pub name: String,
    #[serde(flatten)]
    pub agg_type: AggregationType,
    /// Nested sub-aggregations (computed per bucket)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggs: Option<Vec<AggregationRequest>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AggregationType {
    Count,
    Min {
        field: String,
    },
    Max {
        field: String,
    },
    Sum {
        field: String,
    },
    Avg {
        field: String,
    },
    Stats {
        field: String,
    },
    Terms {
        field: String,
        size: Option<usize>,
    },
    Histogram {
        field: String,
        interval: f64,
        #[serde(default)]
        min_doc_count: Option<u64>,
        #[serde(default)]
        extended_bounds: Option<HistogramBounds>,
    },
    DateHistogram {
        field: String,
        calendar_interval: String,
        #[serde(default)]
        min_doc_count: Option<u64>,
    },
    /// Percentiles metric aggregation
    Percentiles {
        field: String,
        /// Which percentiles to compute (default: [1, 5, 25, 50, 75, 95, 99])
        #[serde(default = "default_percents")]
        percents: Vec<f64>,
    },
    /// Range bucket aggregation with custom boundaries
    Range {
        field: String,
        ranges: Vec<RangeEntry>,
    },
    /// Filter aggregation — narrows context via a query
    Filter {
        #[serde(alias = "query")]
        filter: String,
    },
    /// Multiple named filters
    Filters {
        filters: HashMap<String, String>,
    },
    /// Global aggregation — ignores query filter, runs on all docs
    Global {},
}

fn default_percents() -> Vec<f64> {
    vec![1.0, 5.0, 25.0, 50.0, 75.0, 95.0, 99.0]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramBounds {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeEntry {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub from: Option<f64>,
    #[serde(default)]
    pub to: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResult {
    pub name: String,
    #[serde(flatten)]
    pub value: AggregationValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AggregationValue {
    Single(f64),
    Stats(StatsResult),
    Percentiles(PercentilesResult),
    Buckets(Vec<Bucket>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResult {
    pub count: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sum: Option<f64>,
    pub avg: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PercentilesResult {
    /// Map of percentile -> value, e.g. {"50.0": 12.5, "95.0": 42.0}
    pub values: HashMap<String, Option<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub key: String,
    pub doc_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_aggs: Option<Vec<AggregationResult>>,
}
