//! Elasticsearch Query DSL types
//!
//! These types represent the subset of ES Query DSL that Prism supports.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Root ES search request body
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EsSearchRequest {
    /// The query to execute
    #[serde(default)]
    pub query: Option<EsQuery>,

    /// Starting offset (default 0)
    #[serde(default)]
    pub from: Option<usize>,

    /// Maximum number of results (default 10)
    #[serde(default)]
    pub size: Option<usize>,

    /// Fields to return in _source
    #[serde(default, rename = "_source")]
    pub source: Option<SourceFilter>,

    /// Aggregations
    #[serde(default, alias = "aggregations")]
    pub aggs: Option<HashMap<String, EsAggregation>>,

    /// Sort order
    #[serde(default)]
    pub sort: Option<Vec<SortClause>>,

    /// Highlighting configuration
    #[serde(default)]
    pub highlight: Option<EsHighlight>,

    /// Track total hits exactly
    #[serde(default)]
    pub track_total_hits: Option<TrackTotalHits>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TrackTotalHits {
    Bool(bool),
    Count(usize),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SourceFilter {
    Bool(bool),
    Fields(Vec<String>),
    Object { includes: Option<Vec<String>>, excludes: Option<Vec<String>> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SortClause {
    Field(String),
    Object(HashMap<String, SortOrder>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SortOrder {
    Simple(String),
    Object { order: String },
}

/// ES Query types
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EsQuery {
    /// Match all documents
    MatchAll(MatchAllQuery),

    /// Match query (analyzed full-text)
    Match(HashMap<String, MatchQuery>),

    /// Match phrase query
    MatchPhrase(HashMap<String, MatchPhraseQuery>),

    /// Multi-match across multiple fields
    MultiMatch(MultiMatchQuery),

    /// Term query (exact match, not analyzed)
    Term(HashMap<String, TermValue>),

    /// Terms query (multiple exact matches)
    Terms(HashMap<String, Vec<Value>>),

    /// Range query
    Range(HashMap<String, RangeParams>),

    /// Bool query (must, should, must_not, filter)
    Bool(BoolQuery),

    /// Exists query
    Exists(ExistsQuery),

    /// Query string (Lucene syntax)
    QueryString(QueryStringQuery),

    /// Simple query string
    SimpleQueryString(SimpleQueryStringQuery),

    /// Wildcard query
    Wildcard(HashMap<String, WildcardParams>),

    /// Prefix query
    Prefix(HashMap<String, PrefixParams>),

    /// IDs query
    Ids(IdsQuery),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MatchAllQuery {
    #[serde(default)]
    pub boost: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MatchQuery {
    Simple(String),
    Object {
        query: String,
        #[serde(default)]
        operator: Option<String>,
        #[serde(default)]
        fuzziness: Option<String>,
        #[serde(default)]
        boost: Option<f32>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MatchPhraseQuery {
    Simple(String),
    Object {
        query: String,
        #[serde(default)]
        slop: Option<u32>,
        #[serde(default)]
        boost: Option<f32>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MultiMatchQuery {
    pub query: String,
    #[serde(default)]
    pub fields: Option<Vec<String>>,
    #[serde(default, rename = "type")]
    pub match_type: Option<String>,
    #[serde(default)]
    pub operator: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TermValue {
    Simple(Value),
    Object { value: Value, boost: Option<f32> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RangeParams {
    #[serde(default)]
    pub gte: Option<Value>,
    #[serde(default)]
    pub gt: Option<Value>,
    #[serde(default)]
    pub lte: Option<Value>,
    #[serde(default)]
    pub lt: Option<Value>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub time_zone: Option<String>,
    #[serde(default)]
    pub boost: Option<f32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BoolQuery {
    #[serde(default)]
    pub must: Option<QueryList>,
    #[serde(default)]
    pub should: Option<QueryList>,
    #[serde(default)]
    pub must_not: Option<QueryList>,
    #[serde(default)]
    pub filter: Option<QueryList>,
    #[serde(default)]
    pub minimum_should_match: Option<MinimumShouldMatch>,
    #[serde(default)]
    pub boost: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum QueryList {
    Single(Box<EsQuery>),
    Multiple(Vec<EsQuery>),
}

impl QueryList {
    pub fn into_vec(self) -> Vec<EsQuery> {
        match self {
            QueryList::Single(q) => vec![*q],
            QueryList::Multiple(v) => v,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MinimumShouldMatch {
    Number(i32),
    Percentage(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExistsQuery {
    pub field: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryStringQuery {
    pub query: String,
    #[serde(default)]
    pub default_field: Option<String>,
    #[serde(default)]
    pub fields: Option<Vec<String>>,
    #[serde(default)]
    pub default_operator: Option<String>,
    #[serde(default)]
    pub analyze_wildcard: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimpleQueryStringQuery {
    pub query: String,
    #[serde(default)]
    pub fields: Option<Vec<String>>,
    #[serde(default)]
    pub default_operator: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum WildcardParams {
    Simple(String),
    Object {
        value: String,
        #[serde(default)]
        boost: Option<f32>,
        #[serde(default)]
        case_insensitive: Option<bool>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PrefixParams {
    Simple(String),
    Object {
        value: String,
        #[serde(default)]
        boost: Option<f32>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdsQuery {
    pub values: Vec<String>,
}

/// ES Highlight configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EsHighlight {
    #[serde(default)]
    pub fields: HashMap<String, HighlightField>,
    #[serde(default)]
    pub pre_tags: Option<Vec<String>>,
    #[serde(default)]
    pub post_tags: Option<Vec<String>>,
    #[serde(default)]
    pub fragment_size: Option<usize>,
    #[serde(default)]
    pub number_of_fragments: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HighlightField {
    #[serde(default)]
    pub pre_tags: Option<Vec<String>>,
    #[serde(default)]
    pub post_tags: Option<Vec<String>>,
    #[serde(default)]
    pub fragment_size: Option<usize>,
    #[serde(default)]
    pub number_of_fragments: Option<usize>,
}

/// ES Aggregation types
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EsAggregation {
    // Metric aggregations
    #[serde(default)]
    pub avg: Option<FieldAgg>,
    #[serde(default)]
    pub sum: Option<FieldAgg>,
    #[serde(default)]
    pub min: Option<FieldAgg>,
    #[serde(default)]
    pub max: Option<FieldAgg>,
    #[serde(default)]
    pub stats: Option<FieldAgg>,
    #[serde(default)]
    pub value_count: Option<FieldAgg>,
    #[serde(default)]
    pub cardinality: Option<FieldAgg>,
    #[serde(default)]
    pub percentiles: Option<PercentilesAgg>,

    // Bucket aggregations
    #[serde(default)]
    pub terms: Option<TermsAgg>,
    #[serde(default)]
    pub histogram: Option<HistogramAgg>,
    #[serde(default)]
    pub date_histogram: Option<DateHistogramAgg>,
    #[serde(default)]
    pub range: Option<RangeAgg>,
    #[serde(default)]
    pub date_range: Option<DateRangeAgg>,
    #[serde(default)]
    pub filter: Option<Box<EsQuery>>,
    #[serde(default)]
    pub filters: Option<FiltersAgg>,
    #[serde(default)]
    pub global: Option<GlobalAgg>,

    // Nested aggregations
    #[serde(default, alias = "aggregations")]
    pub aggs: Option<HashMap<String, EsAggregation>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FieldAgg {
    pub field: String,
    #[serde(default)]
    pub missing: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PercentilesAgg {
    pub field: String,
    #[serde(default)]
    pub percents: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TermsAgg {
    pub field: String,
    #[serde(default)]
    pub size: Option<usize>,
    #[serde(default)]
    pub order: Option<HashMap<String, String>>,
    #[serde(default)]
    pub min_doc_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HistogramAgg {
    pub field: String,
    pub interval: f64,
    #[serde(default)]
    pub min_doc_count: Option<u64>,
    #[serde(default)]
    pub extended_bounds: Option<ExtendedBounds>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtendedBounds {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DateHistogramAgg {
    pub field: String,
    #[serde(alias = "fixed_interval")]
    pub calendar_interval: Option<String>,
    #[serde(default)]
    pub min_doc_count: Option<u64>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RangeAgg {
    pub field: String,
    pub ranges: Vec<RangeBucket>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RangeBucket {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub from: Option<f64>,
    #[serde(default)]
    pub to: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DateRangeAgg {
    pub field: String,
    #[serde(default)]
    pub format: Option<String>,
    pub ranges: Vec<DateRangeBucket>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DateRangeBucket {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FiltersAgg {
    pub filters: HashMap<String, EsQuery>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GlobalAgg {}

/// Multi-search request types
#[derive(Debug, Clone)]
pub struct MSearchRequest {
    pub searches: Vec<MSearchItem>,
}

#[derive(Debug, Clone)]
pub struct MSearchItem {
    pub header: MSearchHeader,
    pub body: EsSearchRequest,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MSearchHeader {
    #[serde(default)]
    pub index: Option<String>,
    #[serde(default)]
    pub preference: Option<String>,
    #[serde(default)]
    pub routing: Option<String>,
}

/// Bulk request types
#[derive(Debug, Clone)]
pub enum BulkAction {
    Index {
        index: String,
        id: Option<String>,
        doc: Value,
    },
    Create {
        index: String,
        id: Option<String>,
        doc: Value,
    },
    Delete {
        index: String,
        id: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BulkActionMeta {
    #[serde(default)]
    pub index: Option<BulkMeta>,
    #[serde(default)]
    pub create: Option<BulkMeta>,
    #[serde(default)]
    pub delete: Option<BulkMeta>,
    #[serde(default)]
    pub update: Option<BulkMeta>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BulkMeta {
    #[serde(rename = "_index")]
    pub index: Option<String>,
    #[serde(rename = "_id")]
    pub id: Option<String>,
}
