//! Query DSL translator from Elasticsearch format to Prism

use crate::error::EsCompatError;
use crate::query::types::*;
use prism::aggregations::{AggregationRequest, AggregationType, HistogramBounds, RangeEntry};
use prism::backends::{HighlightConfig, Query};
use serde_json::Value;
use std::collections::HashMap;

/// Translates Elasticsearch Query DSL to Prism query format
pub struct QueryTranslator;

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
            limit: request.size.unwrap_or(10),
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
                // Already in query string format, pass through
                Ok(qs.query.clone())
            }

            EsQuery::SimpleQueryString(qs) => Ok(qs.query.clone()),

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
            let must_queries = must.clone().into_vec();
            for q in must_queries {
                parts.push(format!("({})", Self::translate_query(&q)?));
            }
        }

        // filter → AND (same as must for scoring purposes in Prism)
        if let Some(filter) = &bool_query.filter {
            let filter_queries = filter.clone().into_vec();
            for q in filter_queries {
                parts.push(format!("({})", Self::translate_query(&q)?));
            }
        }

        // must_not → NOT
        if let Some(must_not) = &bool_query.must_not {
            let must_not_queries = must_not.clone().into_vec();
            for q in must_not_queries {
                parts.push(format!("NOT ({})", Self::translate_query(&q)?));
            }
        }

        // should → OR
        if let Some(should) = &bool_query.should {
            let should_queries = should.clone().into_vec();
            if !should_queries.is_empty() {
                let should_parts: Vec<String> = should_queries
                    .iter()
                    .map(Self::translate_query)
                    .collect::<Result<Vec<_>, _>>()?;
                // If there are no must/filter, should becomes the main query
                if parts.is_empty() {
                    parts.push(format!("({})", should_parts.join(" OR ")));
                } else {
                    // With must/filter, should is optional boost
                    parts.push(format!("({})", should_parts.join(" OR ")));
                }
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

    #[test]
    fn test_match_all() {
        let query = EsQuery::MatchAll(MatchAllQuery { boost: None });
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
                    RangeParams {
                        gte: Some(Value::String("now-15m".to_string())),
                        gt: None,
                        lte: None,
                        lt: None,
                        format: None,
                        time_zone: None,
                        boost: None,
                    },
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
    fn test_range_query() {
        let mut fields = HashMap::new();
        fields.insert(
            "age".to_string(),
            RangeParams {
                gte: Some(Value::Number(serde_json::Number::from(18))),
                gt: None,
                lte: Some(Value::Number(serde_json::Number::from(65))),
                lt: None,
                format: None,
                time_zone: None,
                boost: None,
            },
        );

        let query = EsQuery::Range(fields);
        let result = QueryTranslator::translate_query(&query).unwrap();
        assert_eq!(result, "age:[18 TO 65]");
    }

    #[test]
    fn test_aggregation_translation() {
        let mut aggs = HashMap::new();
        aggs.insert(
            "status_count".to_string(),
            EsAggregation {
                terms: Some(TermsAgg {
                    field: "status".to_string(),
                    size: Some(10),
                    order: None,
                    min_doc_count: None,
                }),
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
            },
        );

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
}
