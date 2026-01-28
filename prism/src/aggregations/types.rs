use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationRequest {
    pub name: String,
    #[serde(flatten)]
    pub agg_type: AggregationType,
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
    },
    DateHistogram {
        field: String,
        calendar_interval: String,
    },
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
pub struct Bucket {
    pub key: String,
    pub doc_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_aggs: Option<Vec<AggregationResult>>,
}
