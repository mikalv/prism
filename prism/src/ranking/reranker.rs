//! Reranker trait and types for two-phase ranking
//!
//! Two-phase ranking retrieves a large candidate set cheaply (Phase 1: BM25/vector),
//! then re-ranks the top candidates with an expensive model (Phase 2).
//!
//! This module defines the `Reranker` trait that all re-ranking implementations must satisfy,
//! plus the configuration types used to control reranking behavior.

use crate::backends::SearchResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Options for controlling reranking behavior at search time.
/// Passed alongside the `Query` to avoid modifying that struct.
#[derive(Debug, Clone)]
pub struct RerankOptions {
    /// Whether reranking is enabled
    pub enabled: bool,
    /// Number of candidates to retrieve in Phase 1 (before reranking)
    pub candidates: usize,
    /// Which text fields to extract from documents for reranking
    pub text_fields: Vec<String>,
}

/// Per-request rerank override from the API layer
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RerankRequest {
    /// Enable or disable reranking for this request
    #[serde(default = "default_rerank_enabled")]
    pub enabled: bool,
    /// Override number of Phase 1 candidates
    #[serde(default)]
    pub candidates: Option<usize>,
    /// Override which text fields to use for reranking
    #[serde(default)]
    pub text_fields: Option<Vec<String>>,
}

fn default_rerank_enabled() -> bool {
    true
}

/// Trait for re-ranking search results (Phase 2).
///
/// Implementations score (query, document_text) pairs and return scores
/// in the same order as the input documents.
#[async_trait]
pub trait Reranker: Send + Sync {
    /// Score (query, document_text) pairs. Returns scores in the same order as input.
    async fn rerank(&self, query: &str, documents: &[&str]) -> anyhow::Result<Vec<f32>>;

    /// Score using full SearchResult objects (access to all fields).
    ///
    /// Default implementation: concatenates text from `text_fields` and calls `rerank()`.
    async fn rerank_results(
        &self,
        query: &str,
        results: &[SearchResult],
        text_fields: &[String],
    ) -> anyhow::Result<Vec<f32>> {
        let texts: Vec<String> = results
            .iter()
            .map(|r| extract_text_from_result(r, text_fields))
            .collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        self.rerank(query, &text_refs).await
    }

    /// Human-readable name for this reranker
    fn name(&self) -> &str;
}

/// Extract and concatenate text from specified fields of a SearchResult.
pub fn extract_text_from_result(result: &SearchResult, text_fields: &[String]) -> String {
    if text_fields.is_empty() {
        // Fall back to concatenating all string fields
        result
            .fields
            .values()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        text_fields
            .iter()
            .filter_map(|field| {
                result
                    .fields
                    .get(field)
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A simple test reranker that returns scores based on string length similarity
    struct MockReranker;

    #[async_trait]
    impl Reranker for MockReranker {
        async fn rerank(&self, query: &str, documents: &[&str]) -> anyhow::Result<Vec<f32>> {
            // Score = 1.0 / (1.0 + |query_len - doc_len|)
            Ok(documents
                .iter()
                .map(|doc| 1.0 / (1.0 + (query.len() as f32 - doc.len() as f32).abs()))
                .collect())
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    #[tokio::test]
    async fn test_reranker_trait_basic() {
        let reranker = MockReranker;
        let scores = reranker
            .rerank("hello", &["hello", "hi", "hello world"])
            .await
            .unwrap();
        assert_eq!(scores.len(), 3);
        // "hello" (len=5) should score highest against query "hello" (len=5)
        assert!(scores[0] > scores[1]);
        assert!(scores[0] > scores[2]);
    }

    #[tokio::test]
    async fn test_reranker_default_rerank_results() {
        let reranker = MockReranker;

        let results = vec![
            SearchResult {
                id: "1".to_string(),
                score: 1.0,
                fields: HashMap::from([("title".to_string(), serde_json::json!("exact"))]),
                highlight: None,
            },
            SearchResult {
                id: "2".to_string(),
                score: 0.8,
                fields: HashMap::from([(
                    "title".to_string(),
                    serde_json::json!("a much longer title here"),
                )]),
                highlight: None,
            },
        ];

        let scores = reranker
            .rerank_results("exact", &results, &["title".to_string()])
            .await
            .unwrap();
        assert_eq!(scores.len(), 2);
        // "exact" (len=5) vs query "exact" (len=5) should score higher than long text
        assert!(scores[0] > scores[1]);
    }

    #[test]
    fn test_extract_text_from_result() {
        let result = SearchResult {
            id: "1".to_string(),
            score: 1.0,
            fields: HashMap::from([
                ("title".to_string(), serde_json::json!("Hello World")),
                ("body".to_string(), serde_json::json!("Some body text")),
                ("count".to_string(), serde_json::json!(42)),
            ]),
            highlight: None,
        };

        // Specific fields
        let text = extract_text_from_result(&result, &["title".to_string(), "body".to_string()]);
        assert_eq!(text, "Hello World Some body text");

        // Empty fields = all string values
        let text = extract_text_from_result(&result, &[]);
        assert!(text.contains("Hello World"));
        assert!(text.contains("Some body text"));
        // Numeric field should not appear
        assert!(!text.contains("42"));
    }
}
