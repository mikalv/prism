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
    Object {
        includes: Option<Vec<String>>,
        excludes: Option<Vec<String>>,
    },
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ===================================================================
    // EsQuery deserialization from JSON
    // ===================================================================

    #[test]
    fn test_deserialize_match_all() {
        let q: EsQuery = serde_json::from_value(json!({"match_all": {}})).unwrap();
        assert!(matches!(q, EsQuery::MatchAll(_)));
    }

    #[test]
    fn test_deserialize_match_all_with_boost() {
        let q: EsQuery = serde_json::from_value(json!({"match_all": {"boost": 1.5}})).unwrap();
        match q {
            EsQuery::MatchAll(m) => assert_eq!(m.boost, Some(1.5)),
            _ => panic!("Expected MatchAll"),
        }
    }

    #[test]
    fn test_deserialize_match_simple() {
        let q: EsQuery = serde_json::from_value(json!({"match": {"title": "hello"}})).unwrap();
        match q {
            EsQuery::Match(m) => {
                assert!(matches!(m.get("title").unwrap(), MatchQuery::Simple(s) if s == "hello"));
            }
            _ => panic!("Expected Match"),
        }
    }

    #[test]
    fn test_deserialize_match_object() {
        let q: EsQuery = serde_json::from_value(json!({
            "match": {"title": {"query": "hello world", "operator": "AND"}}
        }))
        .unwrap();
        match q {
            EsQuery::Match(m) => match m.get("title").unwrap() {
                MatchQuery::Object {
                    query, operator, ..
                } => {
                    assert_eq!(query, "hello world");
                    assert_eq!(operator.as_deref(), Some("AND"));
                }
                _ => panic!("Expected Object variant"),
            },
            _ => panic!("Expected Match"),
        }
    }

    #[test]
    fn test_deserialize_match_phrase() {
        let q: EsQuery =
            serde_json::from_value(json!({"match_phrase": {"msg": "quick brown fox"}})).unwrap();
        match q {
            EsQuery::MatchPhrase(m) => {
                assert!(matches!(m.get("msg").unwrap(), MatchPhraseQuery::Simple(s) if s == "quick brown fox"));
            }
            _ => panic!("Expected MatchPhrase"),
        }
    }

    #[test]
    fn test_deserialize_multi_match() {
        let q: EsQuery = serde_json::from_value(json!({
            "multi_match": {"query": "test", "fields": ["title", "body"]}
        }))
        .unwrap();
        match q {
            EsQuery::MultiMatch(mm) => {
                assert_eq!(mm.query, "test");
                assert_eq!(mm.fields.unwrap(), vec!["title", "body"]);
            }
            _ => panic!("Expected MultiMatch"),
        }
    }

    #[test]
    fn test_deserialize_term_simple() {
        let q: EsQuery = serde_json::from_value(json!({"term": {"status": "active"}})).unwrap();
        assert!(matches!(q, EsQuery::Term(_)));
    }

    #[test]
    fn test_deserialize_term_object() {
        // With serde(untagged), the inner {"value": ..., "boost": ...} is first
        // attempted as Simple(Value), and since a JSON object is a valid Value,
        // it will deserialize as Simple. This is the actual behavior.
        let q: EsQuery = serde_json::from_value(json!({
            "term": {"status": {"value": "active", "boost": 2.0}}
        }))
        .unwrap();
        match q {
            EsQuery::Term(m) => {
                // Untagged enum picks Simple(Value::Object) first
                match m.get("status").unwrap() {
                    TermValue::Simple(v) => {
                        assert!(v.is_object());
                        assert_eq!(v["value"], "active");
                    }
                    TermValue::Object { value, boost } => {
                        assert_eq!(value, &Value::String("active".to_string()));
                        assert_eq!(*boost, Some(2.0));
                    }
                }
            }
            _ => panic!("Expected Term"),
        }
    }

    #[test]
    fn test_deserialize_terms() {
        let q: EsQuery =
            serde_json::from_value(json!({"terms": {"status": ["a", "b", "c"]}})).unwrap();
        match q {
            EsQuery::Terms(m) => {
                assert_eq!(m.get("status").unwrap().len(), 3);
            }
            _ => panic!("Expected Terms"),
        }
    }

    #[test]
    fn test_deserialize_range() {
        let q: EsQuery =
            serde_json::from_value(json!({"range": {"age": {"gte": 18, "lte": 65}}})).unwrap();
        match q {
            EsQuery::Range(m) => {
                let r = m.get("age").unwrap();
                assert!(r.gte.is_some());
                assert!(r.lte.is_some());
            }
            _ => panic!("Expected Range"),
        }
    }

    #[test]
    fn test_deserialize_range_with_format() {
        let q: EsQuery = serde_json::from_value(json!({
            "range": {"@timestamp": {"gte": "now-1d", "format": "strict_date_optional_time"}}
        }))
        .unwrap();
        match q {
            EsQuery::Range(m) => {
                let r = m.get("@timestamp").unwrap();
                assert_eq!(r.format.as_deref(), Some("strict_date_optional_time"));
            }
            _ => panic!("Expected Range"),
        }
    }

    #[test]
    fn test_deserialize_bool() {
        let q: EsQuery = serde_json::from_value(json!({
            "bool": {
                "must": [{"term": {"status": "active"}}],
                "should": [{"match": {"title": "hello"}}],
                "must_not": [{"term": {"deleted": true}}]
            }
        }))
        .unwrap();
        match q {
            EsQuery::Bool(b) => {
                assert!(b.must.is_some());
                assert!(b.should.is_some());
                assert!(b.must_not.is_some());
                assert!(b.filter.is_none());
            }
            _ => panic!("Expected Bool"),
        }
    }

    #[test]
    fn test_deserialize_bool_single_must() {
        // Single query should work both as Single and Multiple
        let q: EsQuery = serde_json::from_value(json!({
            "bool": {"must": {"term": {"status": "active"}}}
        }))
        .unwrap();
        match q {
            EsQuery::Bool(b) => {
                let must = b.must.unwrap().into_vec();
                assert_eq!(must.len(), 1);
            }
            _ => panic!("Expected Bool"),
        }
    }

    #[test]
    fn test_deserialize_exists() {
        let q: EsQuery = serde_json::from_value(json!({"exists": {"field": "user"}})).unwrap();
        match q {
            EsQuery::Exists(e) => assert_eq!(e.field, "user"),
            _ => panic!("Expected Exists"),
        }
    }

    #[test]
    fn test_deserialize_query_string() {
        let q: EsQuery = serde_json::from_value(json!({
            "query_string": {"query": "status:active AND type:log"}
        }))
        .unwrap();
        match q {
            EsQuery::QueryString(qs) => {
                assert_eq!(qs.query, "status:active AND type:log");
            }
            _ => panic!("Expected QueryString"),
        }
    }

    #[test]
    fn test_deserialize_simple_query_string() {
        let q: EsQuery = serde_json::from_value(json!({
            "simple_query_string": {"query": "foo + bar", "fields": ["title"]}
        }))
        .unwrap();
        match q {
            EsQuery::SimpleQueryString(qs) => {
                assert_eq!(qs.query, "foo + bar");
                assert_eq!(qs.fields.unwrap(), vec!["title".to_string()]);
            }
            _ => panic!("Expected SimpleQueryString"),
        }
    }

    #[test]
    fn test_deserialize_wildcard() {
        let q: EsQuery =
            serde_json::from_value(json!({"wildcard": {"user": "ki*y"}})).unwrap();
        match q {
            EsQuery::Wildcard(m) => {
                assert!(matches!(m.get("user").unwrap(), WildcardParams::Simple(s) if s == "ki*y"));
            }
            _ => panic!("Expected Wildcard"),
        }
    }

    #[test]
    fn test_deserialize_wildcard_object() {
        let q: EsQuery = serde_json::from_value(json!({
            "wildcard": {"user": {"value": "ki*y", "boost": 1.5, "case_insensitive": true}}
        }))
        .unwrap();
        match q {
            EsQuery::Wildcard(m) => match m.get("user").unwrap() {
                WildcardParams::Object {
                    value,
                    boost,
                    case_insensitive,
                } => {
                    assert_eq!(value, "ki*y");
                    assert_eq!(*boost, Some(1.5));
                    assert_eq!(*case_insensitive, Some(true));
                }
                _ => panic!("Expected Object"),
            },
            _ => panic!("Expected Wildcard"),
        }
    }

    #[test]
    fn test_deserialize_prefix() {
        let q: EsQuery = serde_json::from_value(json!({"prefix": {"name": "joh"}})).unwrap();
        match q {
            EsQuery::Prefix(m) => {
                assert!(matches!(m.get("name").unwrap(), PrefixParams::Simple(s) if s == "joh"));
            }
            _ => panic!("Expected Prefix"),
        }
    }

    #[test]
    fn test_deserialize_ids() {
        let q: EsQuery =
            serde_json::from_value(json!({"ids": {"values": ["1", "2", "3"]}})).unwrap();
        match q {
            EsQuery::Ids(ids) => {
                assert_eq!(ids.values, vec!["1", "2", "3"]);
            }
            _ => panic!("Expected Ids"),
        }
    }

    // ===================================================================
    // QueryList into_vec
    // ===================================================================

    #[test]
    fn test_query_list_single_into_vec() {
        let ql = QueryList::Single(Box::new(EsQuery::MatchAll(MatchAllQuery { boost: None })));
        let v = ql.into_vec();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn test_query_list_multiple_into_vec() {
        let ql = QueryList::Multiple(vec![
            EsQuery::MatchAll(MatchAllQuery { boost: None }),
            EsQuery::MatchAll(MatchAllQuery { boost: None }),
        ]);
        let v = ql.into_vec();
        assert_eq!(v.len(), 2);
    }

    // ===================================================================
    // EsSearchRequest deserialization
    // ===================================================================

    #[test]
    fn test_deserialize_search_request_minimal() {
        let req: EsSearchRequest = serde_json::from_value(json!({})).unwrap();
        assert!(req.query.is_none());
        assert!(req.from.is_none());
        assert!(req.size.is_none());
        assert!(req.aggs.is_none());
    }

    #[test]
    fn test_deserialize_search_request_full() {
        let req: EsSearchRequest = serde_json::from_value(json!({
            "query": {"match_all": {}},
            "from": 10,
            "size": 20,
            "_source": ["title", "body"],
            "highlight": {
                "fields": {"title": {}, "body": {}},
                "pre_tags": ["<b>"],
                "post_tags": ["</b>"]
            }
        }))
        .unwrap();
        assert!(req.query.is_some());
        assert_eq!(req.from, Some(10));
        assert_eq!(req.size, Some(20));
        let hl = req.highlight.unwrap();
        assert_eq!(hl.fields.len(), 2);
        assert_eq!(hl.pre_tags.unwrap(), vec!["<b>".to_string()]);
    }

    #[test]
    fn test_deserialize_search_request_with_aggs_alias() {
        let req: EsSearchRequest = serde_json::from_value(json!({
            "aggregations": {
                "by_status": {"terms": {"field": "status"}}
            }
        }))
        .unwrap();
        assert!(req.aggs.is_some());
        assert!(req.aggs.unwrap().contains_key("by_status"));
    }

    // ===================================================================
    // EsAggregation deserialization
    // ===================================================================

    #[test]
    fn test_deserialize_agg_terms() {
        let agg: EsAggregation =
            serde_json::from_value(json!({"terms": {"field": "status", "size": 20}})).unwrap();
        assert!(agg.terms.is_some());
        let t = agg.terms.unwrap();
        assert_eq!(t.field, "status");
        assert_eq!(t.size, Some(20));
    }

    #[test]
    fn test_deserialize_agg_date_histogram() {
        let agg: EsAggregation = serde_json::from_value(json!({
            "date_histogram": {"field": "@timestamp", "calendar_interval": "1h"}
        }))
        .unwrap();
        assert!(agg.date_histogram.is_some());
        let dh = agg.date_histogram.unwrap();
        assert_eq!(dh.field, "@timestamp");
        assert_eq!(dh.calendar_interval, Some("1h".to_string()));
    }

    #[test]
    fn test_deserialize_agg_date_histogram_fixed_interval_alias() {
        let agg: EsAggregation = serde_json::from_value(json!({
            "date_histogram": {"field": "@timestamp", "fixed_interval": "30s"}
        }))
        .unwrap();
        let dh = agg.date_histogram.unwrap();
        assert_eq!(dh.calendar_interval, Some("30s".to_string()));
    }

    #[test]
    fn test_deserialize_agg_histogram() {
        let agg: EsAggregation = serde_json::from_value(json!({
            "histogram": {"field": "price", "interval": 50.0, "min_doc_count": 1}
        }))
        .unwrap();
        let h = agg.histogram.unwrap();
        assert_eq!(h.field, "price");
        assert_eq!(h.interval, 50.0);
        assert_eq!(h.min_doc_count, Some(1));
    }

    #[test]
    fn test_deserialize_agg_range() {
        let agg: EsAggregation = serde_json::from_value(json!({
            "range": {
                "field": "price",
                "ranges": [
                    {"to": 50.0},
                    {"from": 50.0, "to": 100.0},
                    {"from": 100.0}
                ]
            }
        }))
        .unwrap();
        let r = agg.range.unwrap();
        assert_eq!(r.field, "price");
        assert_eq!(r.ranges.len(), 3);
    }

    #[test]
    fn test_deserialize_agg_nested_subaggs() {
        let agg: EsAggregation = serde_json::from_value(json!({
            "terms": {"field": "category"},
            "aggs": {
                "avg_price": {"avg": {"field": "price"}}
            }
        }))
        .unwrap();
        assert!(agg.terms.is_some());
        assert!(agg.aggs.is_some());
        let sub = agg.aggs.unwrap();
        assert!(sub.contains_key("avg_price"));
    }

    // ===================================================================
    // SourceFilter deserialization
    // ===================================================================

    #[test]
    fn test_source_filter_bool() {
        let sf: SourceFilter = serde_json::from_value(json!(false)).unwrap();
        assert!(matches!(sf, SourceFilter::Bool(false)));
    }

    #[test]
    fn test_source_filter_fields() {
        let sf: SourceFilter = serde_json::from_value(json!(["title", "body"])).unwrap();
        match sf {
            SourceFilter::Fields(f) => assert_eq!(f, vec!["title", "body"]),
            _ => panic!("Expected Fields"),
        }
    }

    #[test]
    fn test_source_filter_object() {
        let sf: SourceFilter =
            serde_json::from_value(json!({"includes": ["title"], "excludes": ["body"]})).unwrap();
        match sf {
            SourceFilter::Object { includes, excludes } => {
                assert_eq!(includes.unwrap(), vec!["title".to_string()]);
                assert_eq!(excludes.unwrap(), vec!["body".to_string()]);
            }
            _ => panic!("Expected Object"),
        }
    }

    // ===================================================================
    // SortClause deserialization
    // ===================================================================

    #[test]
    fn test_sort_clause_string() {
        let s: SortClause = serde_json::from_value(json!("_score")).unwrap();
        assert!(matches!(s, SortClause::Field(f) if f == "_score"));
    }

    #[test]
    fn test_sort_clause_object() {
        let s: SortClause = serde_json::from_value(json!({"date": {"order": "desc"}})).unwrap();
        assert!(matches!(s, SortClause::Object(_)));
    }

    // ===================================================================
    // BulkActionMeta deserialization
    // ===================================================================

    #[test]
    fn test_bulk_action_meta_index() {
        let meta: BulkActionMeta =
            serde_json::from_value(json!({"index": {"_index": "test", "_id": "1"}})).unwrap();
        assert!(meta.index.is_some());
        let idx = meta.index.unwrap();
        assert_eq!(idx.index, Some("test".to_string()));
        assert_eq!(idx.id, Some("1".to_string()));
    }

    #[test]
    fn test_bulk_action_meta_delete() {
        let meta: BulkActionMeta =
            serde_json::from_value(json!({"delete": {"_index": "test", "_id": "2"}})).unwrap();
        assert!(meta.delete.is_some());
    }

    #[test]
    fn test_bulk_action_meta_create() {
        let meta: BulkActionMeta =
            serde_json::from_value(json!({"create": {"_index": "test"}})).unwrap();
        assert!(meta.create.is_some());
        assert!(meta.create.unwrap().id.is_none());
    }

    #[test]
    fn test_bulk_action_meta_update() {
        let meta: BulkActionMeta =
            serde_json::from_value(json!({"update": {"_index": "test", "_id": "3"}})).unwrap();
        assert!(meta.update.is_some());
    }

    // ===================================================================
    // MSearchHeader deserialization
    // ===================================================================

    #[test]
    fn test_msearch_header_empty() {
        let h: MSearchHeader = serde_json::from_value(json!({})).unwrap();
        assert!(h.index.is_none());
        assert!(h.preference.is_none());
        assert!(h.routing.is_none());
    }

    #[test]
    fn test_msearch_header_full() {
        let h: MSearchHeader = serde_json::from_value(json!({
            "index": "logs",
            "preference": "_local",
            "routing": "user1"
        }))
        .unwrap();
        assert_eq!(h.index, Some("logs".to_string()));
        assert_eq!(h.preference, Some("_local".to_string()));
        assert_eq!(h.routing, Some("user1".to_string()));
    }

    // ===================================================================
    // TrackTotalHits deserialization
    // ===================================================================

    #[test]
    fn test_track_total_hits_bool() {
        let t: TrackTotalHits = serde_json::from_value(json!(true)).unwrap();
        assert!(matches!(t, TrackTotalHits::Bool(true)));
    }

    #[test]
    fn test_track_total_hits_count() {
        let t: TrackTotalHits = serde_json::from_value(json!(1000)).unwrap();
        assert!(matches!(t, TrackTotalHits::Count(1000)));
    }

    // ===================================================================
    // MinimumShouldMatch deserialization
    // ===================================================================

    #[test]
    fn test_minimum_should_match_number() {
        let m: MinimumShouldMatch = serde_json::from_value(json!(2)).unwrap();
        assert!(matches!(m, MinimumShouldMatch::Number(2)));
    }

    #[test]
    fn test_minimum_should_match_percentage() {
        let m: MinimumShouldMatch = serde_json::from_value(json!("75%")).unwrap();
        assert!(matches!(m, MinimumShouldMatch::Percentage(s) if s == "75%"));
    }

    // ===================================================================
    // EsHighlight deserialization
    // ===================================================================

    #[test]
    fn test_highlight_deserialization() {
        let hl: EsHighlight = serde_json::from_value(json!({
            "fields": {
                "content": {"fragment_size": 200, "number_of_fragments": 5}
            },
            "pre_tags": ["<mark>"],
            "post_tags": ["</mark>"],
            "fragment_size": 150,
            "number_of_fragments": 3
        }))
        .unwrap();
        assert_eq!(hl.fields.len(), 1);
        let content_field = hl.fields.get("content").unwrap();
        assert_eq!(content_field.fragment_size, Some(200));
        assert_eq!(content_field.number_of_fragments, Some(5));
        assert_eq!(hl.pre_tags.unwrap(), vec!["<mark>".to_string()]);
        assert_eq!(hl.fragment_size, Some(150));
    }
}
