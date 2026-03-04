use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    #[serde(default)]
    pub fields: HashMap<String, serde_json::Value>,
}

impl Document {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            fields: HashMap::new(),
        }
    }

    pub fn field(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    #[serde(default)]
    pub fields: HashMap<String, serde_json::Value>,
    pub highlight: Option<HashMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
    pub total: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub collections: usize,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexResponse {
    pub indexed: usize,
    pub failed: usize,
    #[serde(default)]
    pub errors: Vec<IndexError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexError {
    pub doc_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SegmentInfo {
    pub id: String,
    pub doc_count: u32,
    pub deleted_count: u32,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SegmentsInfo {
    pub segments: Vec<SegmentInfo>,
    pub total_docs: u64,
    pub total_deleted: u64,
    pub delete_ratio: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OptimizeResult {
    pub segments_before: usize,
    pub segments_after: usize,
    pub merged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Highlight {
    pub fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_of_fragments: Option<usize>,
}

impl Highlight {
    pub fn new(fields: &[&str]) -> Self {
        Self {
            fields: fields.iter().map(|s| s.to_string()).collect(),
            pre_tag: None,
            post_tag: None,
            fragment_size: None,
            number_of_fragments: None,
        }
    }

    pub fn pre_tag(mut self, tag: impl Into<String>) -> Self {
        self.pre_tag = Some(tag.into());
        self
    }
    pub fn post_tag(mut self, tag: impl Into<String>) -> Self {
        self.post_tag = Some(tag.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_search_results() {
        let json = r#"{"results":[{"id":"1","score":1.5,"fields":{"title":"Test"}}],"total":1}"#;
        let results: SearchResults = serde_json::from_str(json).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(results.results[0].id, "1");
    }

    #[test]
    fn document_builder() {
        let doc = Document::new("1").field("title", "Hello").field("score", 42);
        assert_eq!(doc.id, "1");
        assert_eq!(doc.fields["title"], serde_json::json!("Hello"));
        assert_eq!(doc.fields["score"], serde_json::json!(42));
    }

    #[test]
    fn deserialize_health() {
        let json = r#"{"status":"ok","version":"0.6.6","collections":4,"uptime_secs":100}"#;
        let h: HealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(h.status, "ok");
        assert_eq!(h.collections, 4);
    }
}
