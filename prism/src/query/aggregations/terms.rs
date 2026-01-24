use super::{AggregationResult, Bucket};
use std::collections::HashMap;

/// Compute terms aggregation from field values
pub fn aggregate_terms(field_values: Vec<String>, size: usize) -> AggregationResult {
    let mut counts: HashMap<String, usize> = HashMap::new();

    // Count occurrences
    for value in field_values {
        *counts.entry(value).or_insert(0) += 1;
    }

    // Sort by count descending, take top N
    let mut buckets: Vec<_> = counts
        .into_iter()
        .map(|(key, count)| Bucket { key, count })
        .collect();

    buckets.sort_by(|a, b| b.count.cmp(&a.count));
    buckets.truncate(size);

    AggregationResult {
        field: "".to_string(), // Will be set by caller
        buckets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terms_aggregation() {
        let values = vec![
            "error".to_string(),
            "warning".to_string(),
            "error".to_string(),
            "info".to_string(),
            "error".to_string(),
        ];

        let result = aggregate_terms(values, 10);

        assert_eq!(result.buckets.len(), 3);
        assert_eq!(result.buckets[0].key, "error");
        assert_eq!(result.buckets[0].count, 3);
        assert_eq!(result.buckets[1].count, 1);
    }

    #[test]
    fn test_terms_aggregation_limit() {
        let values = (0..100).map(|i| format!("val{}", i)).collect();
        let result = aggregate_terms(values, 5);
        assert_eq!(result.buckets.len(), 5);
    }
}
