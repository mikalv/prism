use super::{AggregationRequest, AggregationResult, AggregationType};
use crate::Result;

/// Compute facets from Tantivy search results
/// This is a placeholder that will be integrated with actual Tantivy facet collector
pub fn compute_facets(
    requests: Vec<AggregationRequest>,
    _documents: &[std::collections::HashMap<String, serde_json::Value>],
) -> Result<Vec<AggregationResult>> {
    let mut results = Vec::new();

    for req in requests {
        match req.agg_type {
            AggregationType::Terms => {
                // Extract field values from documents and aggregate
                let field_values: Vec<String> = _documents
                    .iter()
                    .filter_map(|doc| {
                        doc.get(&req.field).and_then(|v| match v {
                            serde_json::Value::String(s) => Some(s.clone()),
                            serde_json::Value::Number(n) => Some(n.to_string()),
                            serde_json::Value::Bool(b) => Some(b.to_string()),
                            _ => None,
                        })
                    })
                    .collect();
                let result = super::terms::aggregate_terms(field_values, req.size);
                results.push(AggregationResult {
                    field: req.field,
                    buckets: result.buckets,
                });
            }
            AggregationType::DateHistogram => {
                use chrono::{DateTime, Utc};
                use super::date_histogram::{aggregate_date_histogram, DateInterval};

                let interval = req.interval
                    .as_deref()
                    .and_then(DateInterval::parse_interval)
                    .unwrap_or(DateInterval::Day);

                let timestamps: Vec<DateTime<Utc>> = _documents
                    .iter()
                    .filter_map(|doc| {
                        doc.get(&req.field).and_then(|v| {
                            v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        })
                    })
                    .map(|dt| dt.with_timezone(&Utc))
                    .collect();

                let result = aggregate_date_histogram(timestamps, interval);
                results.push(AggregationResult {
                    field: req.field,
                    buckets: result.buckets,
                });
            }
            AggregationType::Range => {
                // TODO: Implement range aggregation
                results.push(AggregationResult {
                    field: req.field,
                    buckets: vec![],
                });
            }
            AggregationType::Stats => {
                // TODO: Implement stats aggregation
                results.push(AggregationResult {
                    field: req.field,
                    buckets: vec![],
                });
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_compute_facets_terms() {
        let requests = vec![AggregationRequest {
            field: "type".to_string(),
            agg_type: AggregationType::Terms,
            size: 10,
            interval: None,
        }];

        let docs = vec![];
        let results = compute_facets(requests, &docs).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].field, "type");
    }

    #[test]
    fn test_compute_facets_multiple() {
        let requests = vec![
            AggregationRequest {
                field: "type".to_string(),
                agg_type: AggregationType::Terms,
                size: 10,
                interval: None,
            },
            AggregationRequest {
                field: "status".to_string(),
                agg_type: AggregationType::Terms,
                size: 5,
                interval: None,
            },
        ];

        let docs = vec![];
        let results = compute_facets(requests, &docs).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].field, "type");
        assert_eq!(results[1].field, "status");
    }

    #[test]
    fn test_compute_facets_date_histogram() {
        let requests = vec![AggregationRequest {
            field: "timestamp".to_string(),
            agg_type: AggregationType::DateHistogram,
            size: 10,
            interval: None,
        }];

        let docs = vec![];
        let results = compute_facets(requests, &docs).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].field, "timestamp");
    }

    #[test]
    fn test_compute_facets_date_histogram_extracts_timestamps() {
        use chrono::{Duration, Utc};

        let requests = vec![AggregationRequest {
            field: "timestamp".to_string(),
            agg_type: AggregationType::DateHistogram,
            size: 10,
            interval: Some("day".to_string()),
        }];

        let now = Utc::now();
        let yesterday = now - Duration::days(1);

        let mut doc1 = HashMap::new();
        doc1.insert("timestamp".to_string(), serde_json::json!(now.to_rfc3339()));

        let mut doc2 = HashMap::new();
        doc2.insert("timestamp".to_string(), serde_json::json!(now.to_rfc3339()));

        let mut doc3 = HashMap::new();
        doc3.insert("timestamp".to_string(), serde_json::json!(yesterday.to_rfc3339()));

        let docs = vec![doc1, doc2, doc3];
        let results = compute_facets(requests, &docs).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].field, "timestamp");
        assert_eq!(results[0].buckets.len(), 2); // Two different days
    }

    #[test]
    fn test_compute_facets_terms_extracts_values() {
        let requests = vec![AggregationRequest {
            field: "type".to_string(),
            agg_type: AggregationType::Terms,
            size: 10,
            interval: None,
        }];

        let mut doc1 = HashMap::new();
        doc1.insert("type".to_string(), serde_json::json!("error"));

        let mut doc2 = HashMap::new();
        doc2.insert("type".to_string(), serde_json::json!("error"));

        let mut doc3 = HashMap::new();
        doc3.insert("type".to_string(), serde_json::json!("warning"));

        let docs = vec![doc1, doc2, doc3];
        let results = compute_facets(requests, &docs).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].field, "type");
        assert_eq!(results[0].buckets.len(), 2);
        assert_eq!(results[0].buckets[0].key, "error");
        assert_eq!(results[0].buckets[0].count, 2);
        assert_eq!(results[0].buckets[1].key, "warning");
        assert_eq!(results[0].buckets[1].count, 1);
    }
}
