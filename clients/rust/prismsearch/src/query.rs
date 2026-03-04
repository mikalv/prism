use crate::client::Client;
use crate::error::Result;
use crate::models::{Highlight, SearchResults};

/// Builder pattern for search queries.
pub struct Query {
    collection: String,
    query: Option<String>,
    vector: Option<Vec<f32>>,
    fields: Vec<String>,
    limit: usize,
    offset: usize,
    merge_strategy: Option<String>,
    text_weight: Option<f32>,
    vector_weight: Option<f32>,
    highlight: Option<Highlight>,
    min_score: Option<f32>,
    score_function: Option<String>,
    rrf_k: Option<usize>,
    aggregations: Vec<serde_json::Value>,
}

impl Query {
    /// Create a new query for a collection.
    pub fn new(collection: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            query: Some(query.into()),
            vector: None,
            fields: Vec::new(),
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
            min_score: None,
            score_function: None,
            rrf_k: None,
            aggregations: Vec::new(),
        }
    }

    /// Create a query without a text query (for vector-only or aggregation-only).
    pub fn collection(collection: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            query: None,
            vector: None,
            fields: Vec::new(),
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
            highlight: None,
            min_score: None,
            score_function: None,
            rrf_k: None,
            aggregations: Vec::new(),
        }
    }

    pub fn fields(mut self, fields: &[&str]) -> Self {
        self.fields = fields.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }

    pub fn offset(mut self, n: usize) -> Self {
        self.offset = n;
        self
    }

    pub fn vector(mut self, vec: Vec<f32>) -> Self {
        self.vector = Some(vec);
        self
    }

    pub fn min_score(mut self, s: f32) -> Self {
        self.min_score = Some(s);
        self
    }

    pub fn score_function(mut self, expr: impl Into<String>) -> Self {
        self.score_function = Some(expr.into());
        self
    }

    pub fn merge_strategy(mut self, s: impl Into<String>) -> Self {
        self.merge_strategy = Some(s.into());
        self
    }

    pub fn text_weight(mut self, w: f32) -> Self {
        self.text_weight = Some(w);
        self
    }

    pub fn vector_weight(mut self, w: f32) -> Self {
        self.vector_weight = Some(w);
        self
    }

    pub fn rrf_k(mut self, k: usize) -> Self {
        self.rrf_k = Some(k);
        self
    }

    pub fn highlight(mut self, h: Highlight) -> Self {
        self.highlight = Some(h);
        self
    }

    pub fn aggregate(mut self, name: &str, agg: serde_json::Value) -> Self {
        let mut a = agg;
        a.as_object_mut()
            .map(|m| m.insert("name".into(), serde_json::Value::String(name.into())));
        self.aggregations.push(a);
        self
    }

    /// Convert to search request body.
    pub fn to_request_body(&self) -> serde_json::Value {
        let mut body = serde_json::Map::new();
        body.insert("limit".into(), serde_json::json!(self.limit));

        if let Some(q) = &self.query {
            body.insert("query".into(), serde_json::json!(q));
        }
        if let Some(v) = &self.vector {
            body.insert("vector".into(), serde_json::json!(v));
        }
        if !self.fields.is_empty() {
            body.insert("fields".into(), serde_json::json!(self.fields));
        }
        if self.offset > 0 {
            body.insert("offset".into(), serde_json::json!(self.offset));
        }
        if let Some(s) = &self.merge_strategy {
            body.insert("merge_strategy".into(), serde_json::json!(s));
        }
        if let Some(w) = self.text_weight {
            body.insert("text_weight".into(), serde_json::json!(w));
        }
        if let Some(w) = self.vector_weight {
            body.insert("vector_weight".into(), serde_json::json!(w));
        }
        if let Some(h) = &self.highlight {
            body.insert("highlight".into(), serde_json::to_value(h).unwrap());
        }
        if let Some(s) = self.min_score {
            body.insert("min_score".into(), serde_json::json!(s));
        }
        if let Some(f) = &self.score_function {
            body.insert("score_function".into(), serde_json::json!(f));
        }
        if let Some(k) = self.rrf_k {
            body.insert("rrf_k".into(), serde_json::json!(k));
        }

        serde_json::Value::Object(body)
    }

    /// Convert to aggregation request body.
    pub fn to_aggregate_body(&self) -> serde_json::Value {
        let mut body = serde_json::Map::new();
        body.insert(
            "aggregations".into(),
            serde_json::json!(self.aggregations),
        );
        body.insert("scan_limit".into(), serde_json::json!(self.limit));
        if let Some(q) = &self.query {
            body.insert("query".into(), serde_json::json!(q));
        }
        serde_json::Value::Object(body)
    }

    /// Execute search and return typed results.
    pub async fn execute(self, client: &Client) -> Result<SearchResults> {
        let body = self.to_request_body();
        client.search(&self.collection, &body).await
    }

    /// Execute aggregation query.
    pub async fn execute_aggs(self, client: &Client) -> Result<serde_json::Value> {
        let body = self.to_aggregate_body();
        client.aggregate(&self.collection, &body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_query_body() {
        let q = Query::new("products", "headphones").fields(&["title"]).limit(5);
        let body = q.to_request_body();
        assert_eq!(body["query"], "headphones");
        assert_eq!(body["fields"], serde_json::json!(["title"]));
        assert_eq!(body["limit"], 5);
        assert!(body.get("offset").is_none());
    }

    #[test]
    fn vector_query() {
        let q = Query::collection("products").vector(vec![0.1, 0.2, 0.3]);
        let body = q.to_request_body();
        let vec = body["vector"].as_array().unwrap();
        assert_eq!(vec.len(), 3);
        assert!(body.get("query").is_none());
    }

    #[test]
    fn highlight_config() {
        let q = Query::new("products", "test")
            .highlight(Highlight::new(&["title"]).pre_tag("<b>").post_tag("</b>"));
        let body = q.to_request_body();
        assert_eq!(body["highlight"]["fields"], serde_json::json!(["title"]));
        assert_eq!(body["highlight"]["pre_tag"], "<b>");
    }

    #[test]
    fn aggregate_body() {
        let q = Query::collection("products")
            .aggregate("price_stats", serde_json::json!({"type": "stats", "field": "price"}));
        let body = q.to_aggregate_body();
        assert_eq!(body["aggregations"][0]["name"], "price_stats");
        assert_eq!(body["aggregations"][0]["type"], "stats");
    }
}
