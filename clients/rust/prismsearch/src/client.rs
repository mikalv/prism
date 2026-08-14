use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::models::*;

/// Builder for creating a Prism client.
pub struct ClientBuilder {
    base_url: String,
    api_key: Option<String>,
    timeout: Duration,
}

impl ClientBuilder {
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn build(self) -> Result<Client> {
        let mut headers = HeaderMap::new();
        if let Some(key) = &self.api_key {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", key))
                    .map_err(|e| Error::Api {
                        status: 0,
                        message: format!("Invalid API key: {}", e),
                    })?,
            );
        }

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(self.timeout)
            .build()?;

        Ok(Client {
            base_url: self.base_url,
            http,
        })
    }
}

/// Async Prism client.
pub struct Client {
    base_url: String,
    http: reqwest::Client,
}

impl Client {
    /// Create a new client builder.
    pub fn new(base_url: impl Into<String>) -> ClientBuilder {
        ClientBuilder {
            base_url: base_url.into(),
            api_key: None,
            timeout: Duration::from_secs(30),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self.http.get(self.url(path)).send().await?;
        let status = resp.status().as_u16();
        if status >= 400 {
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status,
                message: text,
            });
        }
        Ok(resp.json().await?)
    }

    pub(crate) async fn post_json<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self.http.post(self.url(path)).json(body).send().await?;
        let status = resp.status().as_u16();
        if status >= 400 {
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status,
                message: text,
            });
        }
        Ok(resp.json().await?)
    }

    async fn put_json<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self.http.put(self.url(path)).json(body).send().await?;
        let status = resp.status().as_u16();
        if status >= 400 {
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status,
                message: text,
            });
        }
        Ok(resp.json().await?)
    }

    async fn delete_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self.http.delete(self.url(path)).send().await?;
        let status = resp.status().as_u16();
        if status >= 400 {
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status,
                message: text,
            });
        }
        Ok(resp.json().await?)
    }

    // -- Health --

    pub async fn health(&self) -> Result<HealthResponse> {
        self.get_json("/health").await
    }

    // -- Collections --

    pub async fn list_collections(&self) -> Result<Vec<String>> {
        self.get_json("/admin/collections").await
    }

    pub async fn create_collection(
        &self,
        name: &str,
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.put_json(&format!("/collections/{}", name), schema)
            .await
    }

    pub async fn delete_collection(&self, name: &str) -> Result<serde_json::Value> {
        self.delete_json(&format!("/collections/{}", name)).await
    }

    /// DELETE /collections/:collection/documents/:id
    ///
    /// Idempotent: a missing document yields `"result": "not_found"` (200),
    /// a missing collection yields 404.
    pub async fn delete_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<serde_json::Value> {
        self.delete_json(&format!("/collections/{}/documents/{}", collection, id))
            .await
    }

    /// POST /collections/:collection/_delete_by_query
    ///
    /// Deletes all documents matching `query` (same query-string syntax as
    /// search). Capped at `max_deletes` (default 1000; 0 = no cap).
    pub async fn delete_by_query(
        &self,
        collection: &str,
        query: &str,
        max_deletes: Option<usize>,
    ) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "query": query,
            "max_deletes": max_deletes.unwrap_or(1000),
        });
        self.post_json(&format!("/collections/{}/_delete_by_query", collection), &body)
            .await
    }

    pub async fn get_schema(&self, collection: &str) -> Result<serde_json::Value> {
        self.get_json(&format!("/collections/{}/schema", collection))
            .await
    }

    pub async fn get_stats(&self, collection: &str) -> Result<serde_json::Value> {
        self.get_json(&format!("/collections/{}/stats", collection))
            .await
    }

    // -- Documents --

    pub async fn index(&self, collection: &str, documents: &[Document]) -> Result<IndexResponse> {
        #[derive(Serialize)]
        struct Body<'a> {
            documents: &'a [Document],
        }
        self.post_json(
            &format!("/collections/{}/documents", collection),
            &Body { documents },
        )
        .await
    }

    pub async fn get_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<serde_json::Value> {
        self.get_json(&format!("/collections/{}/documents/{}", collection, id))
            .await
    }

    // -- Search --

    pub async fn search(
        &self,
        collection: &str,
        body: &serde_json::Value,
    ) -> Result<SearchResults> {
        self.post_json(&format!("/collections/{}/search", collection), body)
            .await
    }

    // -- Aggregations --

    pub async fn aggregate(
        &self,
        collection: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.post_json(&format!("/collections/{}/aggregate", collection), body)
            .await
    }

    // -- Suggest --

    pub async fn suggest(
        &self,
        collection: &str,
        prefix: &str,
        field: &str,
    ) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "prefix": prefix,
            "field": field,
            "size": 5,
            "fuzzy": false,
            "max_distance": 2,
        });
        self.post_json(&format!("/collections/{}/_suggest", collection), &body)
            .await
    }

    // -- More Like This --

    pub async fn mlt(
        &self,
        collection: &str,
        body: &serde_json::Value,
    ) -> Result<SearchResults> {
        self.post_json(&format!("/collections/{}/_mlt", collection), body)
            .await
    }

    // -- Multi-search --

    pub async fn multi_search(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.post_json("/_msearch", body).await
    }

    // -- Segments & Optimize --

    pub async fn segments(&self, collection: &str) -> Result<SegmentsInfo> {
        self.get_json(&format!("/collections/{}/segments", collection))
            .await
    }

    pub async fn optimize(&self, collection: &str) -> Result<OptimizeResult> {
        self.post_json(
            &format!("/collections/{}/optimize", collection),
            &serde_json::json!({}),
        )
        .await
    }

    // -- Stats --

    pub async fn cache_stats(&self) -> Result<serde_json::Value> {
        self.get_json("/stats/cache").await
    }

    pub async fn server_info(&self) -> Result<serde_json::Value> {
        self.get_json("/stats/server").await
    }
}
