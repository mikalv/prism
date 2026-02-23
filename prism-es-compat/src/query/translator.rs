//! Query DSL translator from Elasticsearch format to Prism

use crate::error::EsCompatError;
use crate::query::types::*;
use prism::aggregations::{AggregationRequest, AggregationType, HistogramBounds, RangeEntry};
use prism::backends::{HighlightConfig, Query};
use serde_json::Value;
use std::collections::HashMap;

/// Maximum allowed search result limit
const MAX_SEARCH_LIMIT: usize = 10_000;

/// Translates Elasticsearch Query DSL to Prism query format
pub struct QueryTranslator;

/// Maximum length for passthrough query strings to prevent DoS
const MAX_QUERY_STRING_LENGTH: usize = 10_000;

impl QueryTranslator {
    /// Translate an ES search request to Prism Query + aggregations
    pub fn translate(
        request: &EsSearchRequest,
        default_fields: &[String],
    ) -> Result<(Query, Vec<AggregationRequest>), EsCompatError> {
        // Translate query to query string
        let query_string = match &request.query {
            Some(q) => Self::translate_query(q)?,
            None => "*".to_string(), // Match all
        };

        // Translate aggregations
        let aggregations = match &request.aggs {
            Some(aggs) => Self::translate_aggregations(aggs)?,
            None => vec![],
        };

        // Translate highlight config
        let highlight = request.highlight.as_ref().map(Self::translate_highlight);

        let query = Query {
            query_string,
            fields: default_fields.to_vec(),
            limit: request.size.unwrap_or(10).min(MAX_SEARCH_LIMIT),
            offset: request.from.unwrap_or(0),
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight,
            rrf_k: None,
            min_score: None,
            score_function: None,
            skip_ranking: false,
        };

        Ok((query, aggregations))
    }

    /// Translate an ES query to Prism query string
    pub fn translate_query(query: &EsQuery) -> Result<String, EsCompatError> {
        match query {
            EsQuery::MatchAll(_) => Ok("*".to_string()),

            EsQuery::Match(fields) => {
                let mut parts = vec![];
                for (field, match_query) in fields {
                    let value = match match_query {
                        MatchQuery::Simple(s) => s.clone(),
                        MatchQuery::Object { query, .. } => query.clone(),
                    };
                    parts.push(format!("{}:{}", field, escape_value(&value)));
                }
                Ok(parts.join(" AND "))
            }

            EsQuery::MatchPhrase(fields) => {
                let mut parts = vec![];
                for (field, match_query) in fields {
                    let value = match match_query {
                        MatchPhraseQuery::Simple(s) => s.clone(),
                        MatchPhraseQuery::Object { query, .. } => query.clone(),
                    };
                    parts.push(format!("{}:\"{}\"", field, escape_phrase(&value)));
                }
                Ok(parts.join(" AND "))
            }

            EsQuery::MultiMatch(mm) => {
                let fields = mm.fields.as_deref().unwrap_or(&[]);
                if fields.is_empty() {
                    // Search all fields
                    Ok(escape_value(&mm.query))
                } else {
                    let parts: Vec<String> = fields
                        .iter()
                        .map(|f| format!("{}:{}", f, escape_value(&mm.query)))
                        .collect();
                    let op = mm.operator.as_deref().unwrap_or("OR");
                    Ok(parts.join(&format!(" {} ", op)))
                }
            }

            EsQuery::Term(fields) => {
                let mut parts = vec![];
                for (field, term_val) in fields {
                    let value = match term_val {
                        TermValue::Simple(v) => value_to_string(v),
                        TermValue::Object { value, .. } => value_to_string(value),
                    };
                    parts.push(format!("{}:{}", field, escape_value(&value)));
                }
                Ok(parts.join(" AND "))
            }

            EsQuery::Terms(fields) => {
                let mut parts = vec![];
                for (field, values) in fields {
                    let value_parts: Vec<String> = values
                        .iter()
                        .map(|v| format!("{}:{}", field, escape_value(&value_to_string(v))))
                        .collect();
                    parts.push(format!("({})", value_parts.join(" OR ")));
                }
                Ok(parts.join(" AND "))
            }

            EsQuery::Range(fields) => {
                let mut parts = vec![];
                for (field, params) in fields {
                    let range = Self::translate_range(field, params)?;
                    parts.push(range);
                }
                Ok(parts.join(" AND "))
            }

            EsQuery::Bool(bool_query) => Self::translate_bool(bool_query),

            EsQuery::Exists(exists) => Ok(format!("{field}:*", field = exists.field)),

            EsQuery::QueryString(qs) => {
                // Validate length to prevent DoS from crafted queries
                if qs.query.len() > MAX_QUERY_STRING_LENGTH {
                    return Err(EsCompatError::InvalidQuery(format!(
                        "query_string exceeds maximum length of {} characters",
                        MAX_QUERY_STRING_LENGTH
                    )));
                }
                Ok(qs.query.clone())
            }

            EsQuery::SimpleQueryString(qs) => {
                if qs.query.len() > MAX_QUERY_STRING_LENGTH {
                    return Err(EsCompatError::InvalidQuery(format!(
                        "simple_query_string exceeds maximum length of {} characters",
                        MAX_QUERY_STRING_LENGTH
                    )));
                }
                Ok(qs.query.clone())
            }

            EsQuery::Wildcard(fields) => {
                let mut parts = vec![];
                for (field, params) in fields {
                    let pattern = match params {
                        WildcardParams::Simple(s) => s.clone(),
                        WildcardParams::Object { value, .. } => value.clone(),
                    };
                    parts.push(format!("{}:{}", field, pattern));
                }
                Ok(parts.join(" AND "))
            }

            EsQuery::Prefix(fields) => {
                let mut parts = vec![];
                for (field, params) in fields {
                    let value = match params {
                        PrefixParams::Simple(s) => s.clone(),
                        PrefixParams::Object { value, .. } => value.clone(),
                    };
                    parts.push(format!("{}:{}*", field, escape_value(&value)));
                }
                Ok(parts.join(" AND "))
            }

            EsQuery::Ids(ids) => {
                let id_parts: Vec<String> = ids
                    .values
                    .iter()
                    .map(|id| format!("_id:{}", escape_value(id)))
                    .collect();
                Ok(format!("({})", id_parts.join(" OR ")))
            }
        }
    }

    fn translate_range(field: &str, params: &RangeParams) -> Result<String, EsCompatError> {
        let lower = params
            .gte
            .as_ref()
            .map(|v| (value_to_string(v), true))
            .or_else(|| params.gt.as_ref().map(|v| (value_to_string(v), false)));

        let upper = params
            .lte
            .as_ref()
            .map(|v| (value_to_string(v), true))
            .or_else(|| params.lt.as_ref().map(|v| (value_to_string(v), false)));

        match (lower, upper) {
            (Some((l, l_inc)), Some((u, u_inc))) => {
                let l_bracket = if l_inc { "[" } else { "{" };
                let u_bracket = if u_inc { "]" } else { "}" };
                Ok(format!(
                    "{}:{}{} TO {}{}",
                    field, l_bracket, l, u, u_bracket
                ))
            }
            (Some((l, l_inc)), None) => {
                let l_bracket = if l_inc { "[" } else { "{" };
                Ok(format!("{}:{}{} TO *]", field, l_bracket, l))
            }
            (None, Some((u, u_inc))) => {
                let u_bracket = if u_inc { "]" } else { "}" };
                Ok(format!("{}:[* TO {}{}", field, u, u_bracket))
            }
            (None, None) => Err(EsCompatError::InvalidQuery(
                "Range query must have at least one bound".to_string(),
            )),
        }
    }

    fn translate_bool(bool_query: &BoolQuery) -> Result<String, EsCompatError> {
        let mut parts = vec![];

        // must → AND
        if let Some(must) = &bool_query.must {
            for q in must.iter() {
                parts.push(format!("({})", Self::translate_query(q)?));
            }
        }

        // filter → AND (same as must for scoring purposes in Prism)
        if let Some(filter) = &bool_query.filter {
            for q in filter.iter() {
                parts.push(format!("({})", Self::translate_query(q)?));
            }
        }

        // must_not → NOT
        if let Some(must_not) = &bool_query.must_not {
            for q in must_not.iter() {
                parts.push(format!("NOT ({})", Self::translate_query(q)?));
            }
        }

        // should → OR
        if let Some(should) = &bool_query.should {
            let should_parts: Vec<String> = should
                .iter()
                .map(Self::translate_query)
                .collect::<Result<Vec<_>, _>>()?;
            if !should_parts.is_empty() {
                // If there are no must/filter, should becomes the main query
                parts.push(format!("({})", should_parts.join(" OR ")));
            }
        }

        if parts.is_empty() {
            Ok("*".to_string())
        } else {
            Ok(parts.join(" AND "))
        }
    }

    fn translate_highlight(highlight: &EsHighlight) -> HighlightConfig {
        let fields: Vec<String> = highlight.fields.keys().cloned().collect();

        HighlightConfig {
            fields,
            pre_tag: highlight
                .pre_tags
                .as_ref()
                .and_then(|t| t.first().cloned())
                .unwrap_or_else(|| "<em>".to_string()),
            post_tag: highlight
                .post_tags
                .as_ref()
                .and_then(|t| t.first().cloned())
                .unwrap_or_else(|| "</em>".to_string()),
            fragment_size: highlight.fragment_size.unwrap_or(150),
            number_of_fragments: highlight.number_of_fragments.unwrap_or(3),
        }
    }

    /// Translate ES aggregations to Prism aggregation requests
    pub fn translate_aggregations(
        aggs: &HashMap<String, EsAggregation>,
    ) -> Result<Vec<AggregationRequest>, EsCompatError> {
        let mut requests = vec![];

        for (name, agg) in aggs {
            if let Some(req) = Self::translate_single_aggregation(name, agg)? {
                requests.push(req);
            }
        }

        Ok(requests)
    }

    fn translate_single_aggregation(
        name: &str,
        agg: &EsAggregation,
    ) -> Result<Option<AggregationRequest>, EsCompatError> {
        // Translate sub-aggregations first
        let sub_aggs = agg
            .aggs
            .as_ref()
            .map(Self::translate_aggregations)
            .transpose()?;

        // Check metric aggregations
        if let Some(avg) = &agg.avg {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Avg {
                    field: avg.field.clone(),
                },
                aggs: sub_aggs,
            }));
        }

        if let Some(sum) = &agg.sum {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Sum {
                    field: sum.field.clone(),
                },
                aggs: sub_aggs,
            }));
        }

        if let Some(min) = &agg.min {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Min {
                    field: min.field.clone(),
                },
                aggs: sub_aggs,
            }));
        }

        if let Some(max) = &agg.max {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Max {
                    field: max.field.clone(),
                },
                aggs: sub_aggs,
            }));
        }

        if let Some(stats) = &agg.stats {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Stats {
                    field: stats.field.clone(),
                },
                aggs: sub_aggs,
            }));
        }

        if agg.value_count.is_some() {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Count,
                aggs: sub_aggs,
            }));
        }

        if let Some(percentiles) = &agg.percentiles {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Percentiles {
                    field: percentiles.field.clone(),
                    percents: percentiles
                        .percents
                        .clone()
                        .unwrap_or_else(|| vec![1.0, 5.0, 25.0, 50.0, 75.0, 95.0, 99.0]),
                },
                aggs: sub_aggs,
            }));
        }

        // Check bucket aggregations
        if let Some(terms) = &agg.terms {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Terms {
                    field: terms.field.clone(),
                    size: terms.size,
                },
                aggs: sub_aggs,
            }));
        }

        if let Some(histogram) = &agg.histogram {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Histogram {
                    field: histogram.field.clone(),
                    interval: histogram.interval,
                    min_doc_count: histogram.min_doc_count,
                    extended_bounds: histogram.extended_bounds.as_ref().map(|b| HistogramBounds {
                        min: b.min,
                        max: b.max,
                    }),
                },
                aggs: sub_aggs,
            }));
        }

        if let Some(date_histogram) = &agg.date_histogram {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::DateHistogram {
                    field: date_histogram.field.clone(),
                    calendar_interval: date_histogram
                        .calendar_interval
                        .clone()
                        .unwrap_or_else(|| "1d".to_string()),
                    min_doc_count: date_histogram.min_doc_count,
                },
                aggs: sub_aggs,
            }));
        }

        if let Some(range) = &agg.range {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Range {
                    field: range.field.clone(),
                    ranges: range
                        .ranges
                        .iter()
                        .map(|r| RangeEntry {
                            key: r.key.clone(),
                            from: r.from,
                            to: r.to,
                        })
                        .collect(),
                },
                aggs: sub_aggs,
            }));
        }

        if let Some(filter_query) = &agg.filter {
            let filter_str = Self::translate_query(filter_query)?;
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Filter { filter: filter_str },
                aggs: sub_aggs,
            }));
        }

        if let Some(filters) = &agg.filters {
            let mut filter_map = HashMap::new();
            for (key, query) in &filters.filters {
                filter_map.insert(key.clone(), Self::translate_query(query)?);
            }
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Filters {
                    filters: filter_map,
                },
                aggs: sub_aggs,
            }));
        }

        if agg.global.is_some() {
            return Ok(Some(AggregationRequest {
                name: name.to_string(),
                agg_type: AggregationType::Global {},
                aggs: sub_aggs,
            }));
        }

        // If only sub-aggregations, this might be a pure nesting container
        // Return None and let caller handle
        if sub_aggs.is_some() {
            return Err(EsCompatError::UnsupportedAggregation(format!(
                "Aggregation '{}' has no recognized type",
                name
            )));
        }

        Ok(None)
    }
}

fn escape_value(s: &str) -> String {
    // Escape special Lucene characters and wrap in quotes if needed
    if s.contains(|c: char| c.is_whitespace() || "+-&|!(){}[]^\"~*?:\\/".contains(c)) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

fn escape_phrase(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Helper: build an EsAggregation with all fields defaulted to None
    // ---------------------------------------------------------------
    fn empty_agg() -> EsAggregation {
        EsAggregation {
            terms: None,
            avg: None,
            sum: None,
            min: None,
            max: None,
            stats: None,
            value_count: None,
            cardinality: None,
            percentiles: None,
            histogram: None,
            date_histogram: None,
            range: None,
            date_range: None,
            filter: None,
            filters: None,
            global: None,
            aggs: None,
        }
    }

    fn range_params(
        gte: Option<Value>,
        gt: Option<Value>,
        lte: Option<Value>,
        lt: Option<Value>,
    ) -> RangeParams {
        RangeParams {
            gte,
            gt,
            lte,
            lt,
            format: None,
            time_zone: None,
            boost: None,
        }
    }

    // ===================================================================
    // translate_query — basic query types
    // ===================================================================

    #[test]
    fn test_match_all() {
        let query = EsQuery::MatchAll(MatchAllQuery { boost: None });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "*");
    }

    #[test]
    fn test_match_all_with_boost() {
        let query = EsQuery::MatchAll(MatchAllQuery { boost: Some(1.5) });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "*");
    }

    #[test]
    fn test_term_query() {
        let mut fields = HashMap::new();
        fields.insert(
            "status".to_string(),
            TermValue::Simple(Value::String("error".to_string())),
        );
        let query = EsQuery::Term(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "status:error");
    }

    #[test]
    fn test_term_query_numeric() {
        let mut fields = HashMap::new();
        fields.insert(
            "code".to_string(),
            TermValue::Simple(Value::Number(serde_json::Number::from(404))),
        );
        let query = EsQuery::Term(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "code:404");
    }

    #[test]
    fn test_term_query_bool() {
        let mut fields = HashMap::new();
        fields.insert(
            "active".to_string(),
            TermValue::Simple(Value::Bool(true)),
        );
        let query = EsQuery::Term(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "active:true");
    }

    #[test]
    fn test_term_query_null() {
        let mut fields = HashMap::new();
        fields.insert(
            "tag".to_string(),
            TermValue::Simple(Value::Null),
        );
        let query = EsQuery::Term(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "tag:null");
    }

    #[test]
    fn test_term_object_with_boost() {
        let mut fields = HashMap::new();
        fields.insert(
            "status".to_string(),
            TermValue::Object {
                value: Value::String("active".to_string()),
                boost: Some(2.0),
            },
        );
        let query = EsQuery::Term(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "status:active");
    }

    // ===================================================================
    // Match queries
    // ===================================================================

    #[test]
    fn test_match_query() {
        let mut fields = HashMap::new();
        fields.insert(
            "message".to_string(),
            MatchQuery::Simple("test query".to_string()),
        );
        let query = EsQuery::Match(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "message:\"test query\"");
    }

    #[test]
    fn test_match_query_single_word() {
        let mut fields = HashMap::new();
        fields.insert(
            "title".to_string(),
            MatchQuery::Simple("hello".to_string()),
        );
        let query = EsQuery::Match(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "title:hello");
    }

    #[test]
    fn test_match_query_object_form() {
        let mut fields = HashMap::new();
        fields.insert(
            "title".to_string(),
            MatchQuery::Object {
                query: "search terms".to_string(),
                operator: Some("AND".to_string()),
                fuzziness: None,
                boost: None,
            },
        );
        let query = EsQuery::Match(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "title:\"search terms\"");
    }

    // ===================================================================
    // Match phrase queries
    // ===================================================================

    #[test]
    fn test_match_phrase_simple() {
        let mut fields = HashMap::new();
        fields.insert(
            "content".to_string(),
            MatchPhraseQuery::Simple("quick brown fox".to_string()),
        );
        let query = EsQuery::MatchPhrase(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "content:\"quick brown fox\"");
    }

    #[test]
    fn test_match_phrase_object() {
        let mut fields = HashMap::new();
        fields.insert(
            "content".to_string(),
            MatchPhraseQuery::Object {
                query: "exact phrase".to_string(),
                slop: Some(2),
                boost: None,
            },
        );
        let query = EsQuery::MatchPhrase(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "content:\"exact phrase\"");
    }

    #[test]
    fn test_match_phrase_with_quotes() {
        let mut fields = HashMap::new();
        fields.insert(
            "msg".to_string(),
            MatchPhraseQuery::Simple("she said \"hello\"".to_string()),
        );
        let query = EsQuery::MatchPhrase(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "msg:\"she said \\\"hello\\\"\"");
    }

    // ===================================================================
    // Multi-match queries
    // ===================================================================

    #[test]
    fn test_multi_match_no_fields() {
        let query = EsQuery::MultiMatch(MultiMatchQuery {
            query: "search text".to_string(),
            fields: None,
            match_type: None,
            operator: None,
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "\"search text\"");
    }

    #[test]
    fn test_multi_match_empty_fields() {
        let query = EsQuery::MultiMatch(MultiMatchQuery {
            query: "search".to_string(),
            fields: Some(vec![]),
            match_type: None,
            operator: None,
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "search");
    }

    #[test]
    fn test_multi_match_with_fields_default_or() {
        let query = EsQuery::MultiMatch(MultiMatchQuery {
            query: "hello".to_string(),
            fields: Some(vec!["title".to_string(), "body".to_string()]),
            match_type: None,
            operator: None,
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "title:hello OR body:hello");
    }

    #[test]
    fn test_multi_match_with_and_operator() {
        let query = EsQuery::MultiMatch(MultiMatchQuery {
            query: "hello".to_string(),
            fields: Some(vec!["title".to_string(), "body".to_string()]),
            match_type: None,
            operator: Some("AND".to_string()),
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "title:hello AND body:hello");
    }

    // ===================================================================
    // Terms query
    // ===================================================================

    #[test]
    fn test_terms_query() {
        let mut fields = HashMap::new();
        fields.insert(
            "status".to_string(),
            vec![
                Value::String("active".to_string()),
                Value::String("pending".to_string()),
            ],
        );
        let query = EsQuery::Terms(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert!(result.contains("status:active"));
        assert!(result.contains("status:pending"));
        assert!(result.contains(" OR "));
        assert!(result.starts_with('('));
        assert!(result.ends_with(')'));
    }

    #[test]
    fn test_terms_query_numeric() {
        let mut fields = HashMap::new();
        fields.insert(
            "code".to_string(),
            vec![
                Value::Number(serde_json::Number::from(200)),
                Value::Number(serde_json::Number::from(201)),
            ],
        );
        let query = EsQuery::Terms(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert!(result.contains("code:200"));
        assert!(result.contains("code:201"));
    }

    // ===================================================================
    // Range queries
    // ===================================================================

    #[test]
    fn test_range_query() {
        let mut fields = HashMap::new();
        fields.insert(
            "age".to_string(),
            range_params(
                Some(Value::Number(serde_json::Number::from(18))),
                None,
                Some(Value::Number(serde_json::Number::from(65))),
                None,
            ),
        );
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "age:[18 TO 65]");
    }

    #[test]
    fn test_range_gt_lt_exclusive() {
        let mut fields = HashMap::new();
        fields.insert(
            "price".to_string(),
            range_params(
                None,
                Some(Value::Number(serde_json::Number::from(10))),
                None,
                Some(Value::Number(serde_json::Number::from(100))),
            ),
        );
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "price:{10 TO 100}");
    }

    #[test]
    fn test_range_gte_only() {
        let mut fields = HashMap::new();
        fields.insert(
            "score".to_string(),
            range_params(
                Some(Value::Number(serde_json::Number::from(50))),
                None,
                None,
                None,
            ),
        );
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "score:[50 TO *]");
    }

    #[test]
    fn test_range_gt_only() {
        let mut fields = HashMap::new();
        fields.insert(
            "score".to_string(),
            range_params(
                None,
                Some(Value::Number(serde_json::Number::from(50))),
                None,
                None,
            ),
        );
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "score:{50 TO *]");
    }

    #[test]
    fn test_range_lte_only() {
        let mut fields = HashMap::new();
        fields.insert(
            "count".to_string(),
            range_params(
                None,
                None,
                Some(Value::Number(serde_json::Number::from(999))),
                None,
            ),
        );
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "count:[* TO 999]");
    }

    #[test]
    fn test_range_lt_only() {
        let mut fields = HashMap::new();
        fields.insert(
            "count".to_string(),
            range_params(
                None,
                None,
                None,
                Some(Value::Number(serde_json::Number::from(999))),
            ),
        );
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "count:[* TO 999}");
    }

    #[test]
    fn test_range_no_bounds_error() {
        let mut fields = HashMap::new();
        fields.insert("x".to_string(), range_params(None, None, None, None));
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query);
        assert!(result.is_err());
    }

    #[test]
    fn test_range_mixed_gte_lt() {
        let mut fields = HashMap::new();
        fields.insert(
            "val".to_string(),
            range_params(
                Some(Value::Number(serde_json::Number::from(0))),
                None,
                None,
                Some(Value::Number(serde_json::Number::from(100))),
            ),
        );
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "val:[0 TO 100}");
    }

    #[test]
    fn test_range_string_dates() {
        let mut fields = HashMap::new();
        fields.insert(
            "@timestamp".to_string(),
            range_params(
                Some(Value::String("2024-01-01".to_string())),
                None,
                Some(Value::String("2024-12-31".to_string())),
                None,
            ),
        );
        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "@timestamp:[2024-01-01 TO 2024-12-31]");
    }

    // ===================================================================
    // Bool queries
    // ===================================================================

    #[test]
    fn test_bool_query() {
        let bool_query = BoolQuery {
            must: Some(QueryList::Multiple(vec![EsQuery::Term({
                let mut m = HashMap::new();
                m.insert(
                    "level".to_string(),
                    TermValue::Simple(Value::String("error".to_string())),
                );
                m
            })])),
            filter: Some(QueryList::Multiple(vec![EsQuery::Range({
                let mut m = HashMap::new();
                m.insert(
                    "@timestamp".to_string(),
                    range_params(
                        Some(Value::String("now-15m".to_string())),
                        None,
                        None,
                        None,
                    ),
                );
                m
            })])),
            must_not: None,
            should: None,
            minimum_should_match: None,
            boost: None,
        };

        let query = EsQuery::Bool(bool_query);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert!(result.contains("level:error"));
        assert!(result.contains("@timestamp"));
    }

    #[test]
    fn test_bool_empty() {
        let bool_query = BoolQuery::default();
        let query = EsQuery::Bool(bool_query);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "*");
    }

    #[test]
    fn test_bool_must_only() {
        let bool_query = BoolQuery {
            must: Some(QueryList::Multiple(vec![
                EsQuery::Term({
                    let mut m = HashMap::new();
                    m.insert("a".to_string(), TermValue::Simple(Value::String("1".to_string())));
                    m
                }),
                EsQuery::Term({
                    let mut m = HashMap::new();
                    m.insert("b".to_string(), TermValue::Simple(Value::String("2".to_string())));
                    m
                }),
            ])),
            ..Default::default()
        };
        let result = QueryTranslator::translate_query(&EsQuery::Bool(bool_query)).unwrap();
        assert!(result.contains("(a:1)"));
        assert!(result.contains("(b:2)"));
        assert!(result.contains(" AND "));
    }

    #[test]
    fn test_bool_must_not_only() {
        let bool_query = BoolQuery {
            must_not: Some(QueryList::Multiple(vec![EsQuery::Term({
                let mut m = HashMap::new();
                m.insert("status".to_string(), TermValue::Simple(Value::String("deleted".to_string())));
                m
            })])),
            ..Default::default()
        };
        let result = QueryTranslator::translate_query(&EsQuery::Bool(bool_query)).unwrap();
        assert!(result.contains("NOT (status:deleted)"));
    }

    #[test]
    fn test_bool_should_only() {
        let bool_query = BoolQuery {
            should: Some(QueryList::Multiple(vec![
                EsQuery::Term({
                    let mut m = HashMap::new();
                    m.insert("color".to_string(), TermValue::Simple(Value::String("red".to_string())));
                    m
                }),
                EsQuery::Term({
                    let mut m = HashMap::new();
                    m.insert("color".to_string(), TermValue::Simple(Value::String("blue".to_string())));
                    m
                }),
            ])),
            ..Default::default()
        };
        let result = QueryTranslator::translate_query(&EsQuery::Bool(bool_query)).unwrap();
        assert!(result.contains("color:red"));
        assert!(result.contains("color:blue"));
        assert!(result.contains(" OR "));
    }

    #[test]
    fn test_bool_must_and_should() {
        let bool_query = BoolQuery {
            must: Some(QueryList::Single(Box::new(EsQuery::Term({
                let mut m = HashMap::new();
                m.insert("type".to_string(), TermValue::Simple(Value::String("doc".to_string())));
                m
            })))),
            should: Some(QueryList::Multiple(vec![
                EsQuery::Term({
                    let mut m = HashMap::new();
                    m.insert("priority".to_string(), TermValue::Simple(Value::String("high".to_string())));
                    m
                }),
            ])),
            ..Default::default()
        };
        let result = QueryTranslator::translate_query(&EsQuery::Bool(bool_query)).unwrap();
        assert!(result.contains("type:doc"));
        assert!(result.contains("priority:high"));
        assert!(result.contains(" AND "));
    }

    #[test]
    fn test_bool_filter_treated_as_must() {
        let bool_query = BoolQuery {
            filter: Some(QueryList::Single(Box::new(EsQuery::Term({
                let mut m = HashMap::new();
                m.insert("status".to_string(), TermValue::Simple(Value::String("active".to_string())));
                m
            })))),
            ..Default::default()
        };
        let result = QueryTranslator::translate_query(&EsQuery::Bool(bool_query)).unwrap();
        assert!(result.contains("status:active"));
    }

    #[test]
    fn test_bool_all_clauses() {
        let bool_query = BoolQuery {
            must: Some(QueryList::Single(Box::new(EsQuery::MatchAll(MatchAllQuery { boost: None })))),
            filter: Some(QueryList::Single(Box::new(EsQuery::Term({
                let mut m = HashMap::new();
                m.insert("status".to_string(), TermValue::Simple(Value::String("ok".to_string())));
                m
            })))),
            must_not: Some(QueryList::Single(Box::new(EsQuery::Term({
                let mut m = HashMap::new();
                m.insert("deleted".to_string(), TermValue::Simple(Value::Bool(true)));
                m
            })))),
            should: Some(QueryList::Single(Box::new(EsQuery::Term({
                let mut m = HashMap::new();
                m.insert("featured".to_string(), TermValue::Simple(Value::Bool(true)));
                m
            })))),
            minimum_should_match: None,
            boost: None,
        };
        let result = QueryTranslator::translate_query(&EsQuery::Bool(bool_query)).unwrap();
        assert!(result.contains("(*)"), "must match_all");
        assert!(result.contains("status:ok"), "filter");
        assert!(result.contains("NOT (deleted:true)"), "must_not");
        assert!(result.contains("featured:true"), "should");
    }

    // ===================================================================
    // Exists query
    // ===================================================================

    #[test]
    fn test_exists_query() {
        let query = EsQuery::Exists(ExistsQuery {
            field: "user".to_string(),
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "user:*");
    }

    // ===================================================================
    // QueryString & SimpleQueryString
    // ===================================================================

    #[test]
    fn test_query_string() {
        let query = EsQuery::QueryString(QueryStringQuery {
            query: "status:active AND level:error".to_string(),
            default_field: None,
            fields: None,
            default_operator: None,
            analyze_wildcard: None,
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "status:active AND level:error");
    }

    #[test]
    fn test_simple_query_string() {
        let query = EsQuery::SimpleQueryString(SimpleQueryStringQuery {
            query: "foo + bar".to_string(),
            fields: None,
            default_operator: None,
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "foo + bar");
    }

    // ===================================================================
    // Wildcard query
    // ===================================================================

    #[test]
    fn test_wildcard_simple() {
        let mut fields = HashMap::new();
        fields.insert(
            "user".to_string(),
            WildcardParams::Simple("ki*y".to_string()),
        );
        let query = EsQuery::Wildcard(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "user:ki*y");
    }

    #[test]
    fn test_wildcard_object() {
        let mut fields = HashMap::new();
        fields.insert(
            "host".to_string(),
            WildcardParams::Object {
                value: "server-*".to_string(),
                boost: Some(1.0),
                case_insensitive: Some(true),
            },
        );
        let query = EsQuery::Wildcard(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "host:server-*");
    }

    // ===================================================================
    // Prefix query
    // ===================================================================

    #[test]
    fn test_prefix_simple() {
        let mut fields = HashMap::new();
        fields.insert(
            "user.name".to_string(),
            PrefixParams::Simple("joh".to_string()),
        );
        let query = EsQuery::Prefix(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "user.name:joh*");
    }

    #[test]
    fn test_prefix_object() {
        let mut fields = HashMap::new();
        fields.insert(
            "tag".to_string(),
            PrefixParams::Object {
                value: "prod".to_string(),
                boost: Some(2.0),
            },
        );
        let query = EsQuery::Prefix(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "tag:prod*");
    }

    // ===================================================================
    // IDs query
    // ===================================================================

    #[test]
    fn test_ids_single() {
        let query = EsQuery::Ids(IdsQuery {
            values: vec!["abc123".to_string()],
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "(_id:abc123)");
    }

    #[test]
    fn test_ids_multiple() {
        let query = EsQuery::Ids(IdsQuery {
            values: vec!["id1".to_string(), "id2".to_string(), "id3".to_string()],
        });
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert!(result.contains("_id:id1"));
        assert!(result.contains("_id:id2"));
        assert!(result.contains("_id:id3"));
        assert!(result.contains(" OR "));
        assert!(result.starts_with('('));
    }

    // ===================================================================
    // Serde round-trip (JSON -> EsQuery -> translate)
    // ===================================================================

    #[test]
    fn test_query_from_json_match_all() {
        let json = serde_json::json!({"match_all": {}});
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "*");
    }

    #[test]
    fn test_query_from_json_term() {
        let json = serde_json::json!({"term": {"status": "active"}});
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "status:active");
    }

    #[test]
    fn test_query_from_json_match() {
        let json = serde_json::json!({"match": {"message": "quick fox"}});
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "message:\"quick fox\"");
    }

    #[test]
    fn test_query_from_json_bool_complex() {
        let json = serde_json::json!({
            "bool": {
                "must": [
                    {"term": {"type": "log"}},
                    {"match": {"message": "error"}}
                ],
                "must_not": [
                    {"term": {"level": "debug"}}
                ],
                "should": [
                    {"term": {"priority": "high"}}
                ]
            }
        });
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert!(result.contains("type:log"));
        assert!(result.contains("message:error"));
        assert!(result.contains("NOT (level:debug)"));
        assert!(result.contains("priority:high"));
    }

    #[test]
    fn test_query_from_json_range() {
        let json = serde_json::json!({"range": {"price": {"gte": 10, "lt": 100}}});
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "price:[10 TO 100}");
    }

    #[test]
    fn test_query_from_json_exists() {
        let json = serde_json::json!({"exists": {"field": "email"}});
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "email:*");
    }

    #[test]
    fn test_query_from_json_wildcard() {
        let json = serde_json::json!({"wildcard": {"user": "ki*y"}});
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "user:ki*y");
    }

    #[test]
    fn test_query_from_json_prefix() {
        let json = serde_json::json!({"prefix": {"name": "joh"}});
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "name:joh*");
    }

    #[test]
    fn test_query_from_json_ids() {
        let json = serde_json::json!({"ids": {"values": ["a", "b"]}});
        let query: EsQuery = serde_json::from_value(json).unwrap();
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert!(result.contains("_id:a"));
        assert!(result.contains("_id:b"));
    }

    // ===================================================================
    // escape_value / escape_phrase helper tests
    // ===================================================================

    #[test]
    fn test_escape_value_no_specials() {
        assert_eq!(escape_value("simple"), "simple");
    }

    #[test]
    fn test_escape_value_with_space() {
        assert_eq!(escape_value("hello world"), "\"hello world\"");
    }

    #[test]
    fn test_escape_value_with_colon() {
        assert_eq!(escape_value("foo:bar"), "\"foo:bar\"");
    }

    #[test]
    fn test_escape_value_with_quotes() {
        assert_eq!(escape_value("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn test_escape_value_with_backslash() {
        assert_eq!(escape_value("path\\to"), "\"path\\\\to\"");
    }

    #[test]
    fn test_escape_phrase_plain() {
        assert_eq!(escape_phrase("hello world"), "hello world");
    }

    #[test]
    fn test_escape_phrase_with_quotes() {
        assert_eq!(escape_phrase("she said \"hi\""), "she said \\\"hi\\\"");
    }

    #[test]
    fn test_value_to_string_string() {
        assert_eq!(value_to_string(&Value::String("hello".to_string())), "hello");
    }

    #[test]
    fn test_value_to_string_number() {
        assert_eq!(value_to_string(&Value::Number(serde_json::Number::from(42))), "42");
    }

    #[test]
    fn test_value_to_string_bool() {
        assert_eq!(value_to_string(&Value::Bool(false)), "false");
    }

    #[test]
    fn test_value_to_string_null() {
        assert_eq!(value_to_string(&Value::Null), "null");
    }

    #[test]
    fn test_value_to_string_array() {
        let arr = Value::Array(vec![Value::Number(1.into())]);
        assert_eq!(value_to_string(&arr), "[1]");
    }

    // ===================================================================
    // Aggregation translations
    // ===================================================================

    #[test]
    fn test_aggregation_translation() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.terms = Some(TermsAgg {
            field: "status".to_string(),
            size: Some(10),
            order: None,
            min_doc_count: None,
        });
        aggs.insert("status_count".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "status_count");
        match &result[0].agg_type {
            AggregationType::Terms { field, size } => {
                assert_eq!(field, "status");
                assert_eq!(*size, Some(10));
            }
            _ => panic!("Expected terms aggregation"),
        }
    }

    #[test]
    fn test_agg_avg() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.avg = Some(FieldAgg {
            field: "price".to_string(),
            missing: None,
        });
        aggs.insert("avg_price".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "avg_price");
        match &result[0].agg_type {
            AggregationType::Avg { field } => assert_eq!(field, "price"),
            _ => panic!("Expected avg aggregation"),
        }
    }

    #[test]
    fn test_agg_sum() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.sum = Some(FieldAgg {
            field: "amount".to_string(),
            missing: None,
        });
        aggs.insert("total_amount".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Sum { field } => assert_eq!(field, "amount"),
            _ => panic!("Expected sum aggregation"),
        }
    }

    #[test]
    fn test_agg_min() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.min = Some(FieldAgg {
            field: "latency".to_string(),
            missing: None,
        });
        aggs.insert("min_latency".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Min { field } => assert_eq!(field, "latency"),
            _ => panic!("Expected min aggregation"),
        }
    }

    #[test]
    fn test_agg_max() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.max = Some(FieldAgg {
            field: "latency".to_string(),
            missing: None,
        });
        aggs.insert("max_latency".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Max { field } => assert_eq!(field, "latency"),
            _ => panic!("Expected max aggregation"),
        }
    }

    #[test]
    fn test_agg_stats() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.stats = Some(FieldAgg {
            field: "response_time".to_string(),
            missing: None,
        });
        aggs.insert("rt_stats".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Stats { field } => assert_eq!(field, "response_time"),
            _ => panic!("Expected stats aggregation"),
        }
    }

    #[test]
    fn test_agg_value_count() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.value_count = Some(FieldAgg {
            field: "status".to_string(),
            missing: None,
        });
        aggs.insert("total".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].agg_type, AggregationType::Count));
    }

    #[test]
    fn test_agg_percentiles_default() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.percentiles = Some(PercentilesAgg {
            field: "latency".to_string(),
            percents: None,
        });
        aggs.insert("lat_pct".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Percentiles { field, percents } => {
                assert_eq!(field, "latency");
                assert_eq!(percents, &vec![1.0, 5.0, 25.0, 50.0, 75.0, 95.0, 99.0]);
            }
            _ => panic!("Expected percentiles aggregation"),
        }
    }

    #[test]
    fn test_agg_percentiles_custom() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.percentiles = Some(PercentilesAgg {
            field: "latency".to_string(),
            percents: Some(vec![50.0, 99.0]),
        });
        aggs.insert("lat_pct".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        match &result[0].agg_type {
            AggregationType::Percentiles { percents, .. } => {
                assert_eq!(percents, &vec![50.0, 99.0]);
            }
            _ => panic!("Expected percentiles aggregation"),
        }
    }

    #[test]
    fn test_agg_histogram() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.histogram = Some(HistogramAgg {
            field: "price".to_string(),
            interval: 50.0,
            min_doc_count: Some(1),
            extended_bounds: None,
        });
        aggs.insert("price_hist".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Histogram {
                field,
                interval,
                min_doc_count,
                extended_bounds,
            } => {
                assert_eq!(field, "price");
                assert_eq!(*interval, 50.0);
                assert_eq!(*min_doc_count, Some(1));
                assert!(extended_bounds.is_none());
            }
            _ => panic!("Expected histogram aggregation"),
        }
    }

    #[test]
    fn test_agg_histogram_with_bounds() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.histogram = Some(HistogramAgg {
            field: "score".to_string(),
            interval: 10.0,
            min_doc_count: None,
            extended_bounds: Some(ExtendedBounds {
                min: 0.0,
                max: 100.0,
            }),
        });
        aggs.insert("score_hist".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        match &result[0].agg_type {
            AggregationType::Histogram {
                extended_bounds, ..
            } => {
                let bounds = extended_bounds.as_ref().unwrap();
                assert_eq!(bounds.min, 0.0);
                assert_eq!(bounds.max, 100.0);
            }
            _ => panic!("Expected histogram aggregation"),
        }
    }

    #[test]
    fn test_agg_date_histogram() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.date_histogram = Some(DateHistogramAgg {
            field: "@timestamp".to_string(),
            calendar_interval: Some("1h".to_string()),
            min_doc_count: Some(0),
            format: None,
            time_zone: None,
        });
        aggs.insert("ts_hist".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::DateHistogram {
                field,
                calendar_interval,
                min_doc_count,
            } => {
                assert_eq!(field, "@timestamp");
                assert_eq!(calendar_interval, "1h");
                assert_eq!(*min_doc_count, Some(0));
            }
            _ => panic!("Expected date_histogram aggregation"),
        }
    }

    #[test]
    fn test_agg_date_histogram_default_interval() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.date_histogram = Some(DateHistogramAgg {
            field: "@timestamp".to_string(),
            calendar_interval: None,
            min_doc_count: None,
            format: None,
            time_zone: None,
        });
        aggs.insert("ts".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        match &result[0].agg_type {
            AggregationType::DateHistogram {
                calendar_interval, ..
            } => {
                assert_eq!(calendar_interval, "1d");
            }
            _ => panic!("Expected date_histogram"),
        }
    }

    #[test]
    fn test_agg_range() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.range = Some(RangeAgg {
            field: "price".to_string(),
            ranges: vec![
                RangeBucket {
                    key: Some("cheap".to_string()),
                    from: None,
                    to: Some(50.0),
                },
                RangeBucket {
                    key: Some("mid".to_string()),
                    from: Some(50.0),
                    to: Some(100.0),
                },
                RangeBucket {
                    key: Some("expensive".to_string()),
                    from: Some(100.0),
                    to: None,
                },
            ],
        });
        aggs.insert("price_ranges".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Range { field, ranges } => {
                assert_eq!(field, "price");
                assert_eq!(ranges.len(), 3);
                assert_eq!(ranges[0].key.as_deref(), Some("cheap"));
                assert_eq!(ranges[0].to, Some(50.0));
                assert_eq!(ranges[1].from, Some(50.0));
                assert_eq!(ranges[1].to, Some(100.0));
                assert_eq!(ranges[2].from, Some(100.0));
                assert!(ranges[2].to.is_none());
            }
            _ => panic!("Expected range aggregation"),
        }
    }

    #[test]
    fn test_agg_filter() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.filter = Some(Box::new(EsQuery::Term({
            let mut m = HashMap::new();
            m.insert("status".to_string(), TermValue::Simple(Value::String("error".to_string())));
            m
        })));
        aggs.insert("error_docs".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Filter { filter } => {
                assert_eq!(filter, "status:error");
            }
            _ => panic!("Expected filter aggregation"),
        }
    }

    #[test]
    fn test_agg_filters() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        let mut filter_map = HashMap::new();
        filter_map.insert(
            "errors".to_string(),
            EsQuery::Term({
                let mut m = HashMap::new();
                m.insert("level".to_string(), TermValue::Simple(Value::String("error".to_string())));
                m
            }),
        );
        filter_map.insert(
            "warnings".to_string(),
            EsQuery::Term({
                let mut m = HashMap::new();
                m.insert("level".to_string(), TermValue::Simple(Value::String("warn".to_string())));
                m
            }),
        );
        a.filters = Some(FiltersAgg {
            filters: filter_map,
        });
        aggs.insert("by_level".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0].agg_type {
            AggregationType::Filters { filters } => {
                assert_eq!(filters.len(), 2);
                assert_eq!(filters["errors"], "level:error");
                assert_eq!(filters["warnings"], "level:warn");
            }
            _ => panic!("Expected filters aggregation"),
        }
    }

    #[test]
    fn test_agg_global() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.global = Some(GlobalAgg {});
        aggs.insert("all_products".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].agg_type, AggregationType::Global {}));
    }

    #[test]
    fn test_agg_empty() {
        let aggs = HashMap::new();
        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_agg_no_recognized_type_with_subaggs_errors() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        // Has sub-aggs but no aggregation type on the container
        let mut sub = HashMap::new();
        let mut sub_a = empty_agg();
        sub_a.avg = Some(FieldAgg {
            field: "price".to_string(),
            missing: None,
        });
        sub.insert("avg_price".to_string(), sub_a);
        a.aggs = Some(sub);
        aggs.insert("container".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs);
        assert!(result.is_err());
    }

    #[test]
    fn test_agg_unknown_type_returns_none() {
        let mut aggs = HashMap::new();
        let a = empty_agg(); // No fields set, no sub-aggs
        aggs.insert("empty_agg".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_agg_with_sub_aggs() {
        let mut aggs = HashMap::new();
        let mut a = empty_agg();
        a.terms = Some(TermsAgg {
            field: "category".to_string(),
            size: Some(5),
            order: None,
            min_doc_count: None,
        });
        let mut sub = HashMap::new();
        let mut sub_a = empty_agg();
        sub_a.avg = Some(FieldAgg {
            field: "price".to_string(),
            missing: None,
        });
        sub.insert("avg_price".to_string(), sub_a);
        a.aggs = Some(sub);
        aggs.insert("by_cat".to_string(), a);

        let result = QueryTranslator::translate_aggregations(&aggs).unwrap();
        assert_eq!(result.len(), 1);
        let sub_aggs = result[0].aggs.as_ref().unwrap();
        assert_eq!(sub_aggs.len(), 1);
        assert_eq!(sub_aggs[0].name, "avg_price");
    }

    // ===================================================================
    // Full translate() tests
    // ===================================================================

    #[test]
    fn test_translate_no_query_no_aggs() {
        let request = EsSearchRequest {
            query: None,
            from: None,
            size: None,
            source: None,
            aggs: None,
            sort: None,
            highlight: None,
            track_total_hits: None,
        };
        let (query, aggs) = QueryTranslator::translate(&request, &["title".to_string()]).unwrap();
        assert_eq!(query.query_string, "*");
        assert_eq!(query.limit, 10);
        assert_eq!(query.offset, 0);
        assert!(aggs.is_empty());
        assert_eq!(query.fields, vec!["title".to_string()]);
    }

    #[test]
    fn test_translate_with_from_size() {
        let request = EsSearchRequest {
            query: None,
            from: Some(20),
            size: Some(50),
            source: None,
            aggs: None,
            sort: None,
            highlight: None,
            track_total_hits: None,
        };
        let (query, _) = QueryTranslator::translate(&request, &[]).unwrap();
        assert_eq!(query.offset, 20);
        assert_eq!(query.limit, 50);
    }

    #[test]
    fn test_translate_with_highlight() {
        let mut highlight_fields = HashMap::new();
        highlight_fields.insert("content".to_string(), HighlightField::default());
        let request = EsSearchRequest {
            query: None,
            from: None,
            size: None,
            source: None,
            aggs: None,
            sort: None,
            highlight: Some(EsHighlight {
                fields: highlight_fields,
                pre_tags: Some(vec!["<b>".to_string()]),
                post_tags: Some(vec!["</b>".to_string()]),
                fragment_size: Some(200),
                number_of_fragments: Some(5),
            }),
            track_total_hits: None,
        };
        let (query, _) = QueryTranslator::translate(&request, &[]).unwrap();
        let hl = query.highlight.unwrap();
        assert_eq!(hl.fields, vec!["content".to_string()]);
        assert_eq!(hl.pre_tag, "<b>");
        assert_eq!(hl.post_tag, "</b>");
        assert_eq!(hl.fragment_size, 200);
        assert_eq!(hl.number_of_fragments, 5);
    }

    #[test]
    fn test_translate_highlight_defaults() {
        let mut highlight_fields = HashMap::new();
        highlight_fields.insert("body".to_string(), HighlightField::default());
        let request = EsSearchRequest {
            query: None,
            from: None,
            size: None,
            source: None,
            aggs: None,
            sort: None,
            highlight: Some(EsHighlight {
                fields: highlight_fields,
                pre_tags: None,
                post_tags: None,
                fragment_size: None,
                number_of_fragments: None,
            }),
            track_total_hits: None,
        };
        let (query, _) = QueryTranslator::translate(&request, &[]).unwrap();
        let hl = query.highlight.unwrap();
        assert_eq!(hl.pre_tag, "<em>");
        assert_eq!(hl.post_tag, "</em>");
        assert_eq!(hl.fragment_size, 150);
        assert_eq!(hl.number_of_fragments, 3);
    }

    #[test]
    fn test_translate_with_query_and_aggs() {
        let mut agg_map = HashMap::new();
        let mut a = empty_agg();
        a.terms = Some(TermsAgg {
            field: "status".to_string(),
            size: None,
            order: None,
            min_doc_count: None,
        });
        agg_map.insert("by_status".to_string(), a);

        let request = EsSearchRequest {
            query: Some(EsQuery::MatchAll(MatchAllQuery { boost: None })),
            from: Some(0),
            size: Some(5),
            source: None,
            aggs: Some(agg_map),
            sort: None,
            highlight: None,
            track_total_hits: None,
        };
        let (query, aggs) = QueryTranslator::translate(&request, &[]).unwrap();
        assert_eq!(query.query_string, "*");
        assert_eq!(query.limit, 5);
        assert_eq!(aggs.len(), 1);
        assert_eq!(aggs[0].name, "by_status");
    }
}
