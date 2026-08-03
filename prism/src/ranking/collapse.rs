//! Field collapse — keep at most K results per distinct value of a field,
//! preserving score order (Elasticsearch-style `collapse`).
//!
//! Useful when many results share a grouping key (e.g. one hit per
//! `conversation_id` instead of 50 from the same conversation).

use crate::backends::SearchResult;
use serde::Deserialize;
use std::collections::HashMap;

/// Configuration for field collapsing.
#[derive(Debug, Clone, Deserialize)]
pub struct CollapseConfig {
    /// Field whose value defines a group.
    pub field: String,
    /// Maximum results to keep per group (default: 1, Elasticsearch-style).
    #[serde(default = "default_max_per_group")]
    pub max_per_group: usize,
}

fn default_max_per_group() -> usize {
    1
}

/// Collapse score-ordered `results` so each distinct value of `field` appears at
/// most `max_per_group` times. Input order is preserved, so the highest-scoring
/// member of each group is kept. Results missing the field are always kept
/// (they cannot be grouped).
pub fn collapse_results(
    results: Vec<SearchResult>,
    field: &str,
    max_per_group: usize,
) -> Vec<SearchResult> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        match r.fields.get(field) {
            Some(value) => {
                let count = counts.entry(group_key(value)).or_insert(0);
                if *count < max_per_group {
                    out.push(r);
                    *count += 1;
                }
                // otherwise the group is full → drop this (lower-scoring) result
            }
            // No grouping value → cannot collapse; always keep.
            None => out.push(r),
        }
    }
    out
}

/// Stable string key for a field value used to group results.
fn group_key(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(id: &str, score: f32, conv: &str) -> SearchResult {
        let mut fields = HashMap::new();
        fields.insert("conversation_id".to_string(), json!(conv));
        SearchResult {
            id: id.into(),
            score,
            fields,
            highlight: None,
        }
    }

    #[test]
    fn collapse_keeps_top_one_per_group_in_order() {
        let results = vec![
            hit("a", 5.0, "conv1"),
            hit("b", 4.0, "conv1"), // same group, lower score → collapsed away
            hit("c", 3.0, "conv2"),
            hit("d", 2.0, "conv1"), // collapsed away
        ];

        let out = collapse_results(results, "conversation_id", 1);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c"]);
    }

    #[test]
    fn collapse_keeps_up_to_max_per_group() {
        let results = vec![
            hit("a", 5.0, "conv1"),
            hit("b", 4.0, "conv1"),
            hit("c", 3.0, "conv1"), // third in group → dropped when max=2
            hit("d", 2.0, "conv2"),
        ];

        let out = collapse_results(results, "conversation_id", 2);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "d"]);
    }

    #[test]
    fn collapse_keeps_results_missing_the_field() {
        let mut no_field = hit("x", 1.0, "conv1");
        no_field.fields.remove("conversation_id");

        let results = vec![hit("a", 5.0, "conv1"), no_field, hit("b", 4.0, "conv1")];

        let out = collapse_results(results, "conversation_id", 1);

        // "a" kept (first of conv1), "x" kept (no group value), "b" dropped.
        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "x"]);
    }
}
