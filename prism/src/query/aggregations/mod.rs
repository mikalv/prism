pub mod date_histogram;
pub mod facets;
pub mod terms;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationRequest {
    pub field: String,
    pub agg_type: AggregationType,
    pub size: usize,
    /// For date_histogram: "hour", "day", "week", "month", "year"
    #[serde(default)]
    pub interval: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregationType {
    Terms,
    DateHistogram,
    Range,
    Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResult {
    pub field: String,
    pub buckets: Vec<Bucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub key: String,
    pub count: usize,
}
