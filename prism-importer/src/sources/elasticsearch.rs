use async_trait::async_trait;
use base64::Engine;
use futures::Stream;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use std::pin::Pin;
use url::Url;

use super::traits::{ImportSource, SourceDocument};
use crate::error::{ImportError, Result};
use crate::schema::{convert_es_mapping, mapping::EsMappingResponse, SourceSchema};

/// Authentication method for Elasticsearch
#[derive(Debug, Clone)]
pub enum AuthMethod {
    None,
    Basic { user: String, password: String },
    ApiKey(String),
}

pub struct ElasticsearchSource {
    client: reqwest::Client,
    base_url: Url,
    index: String,
    batch_size: usize,
}

impl ElasticsearchSource {
    pub fn new(base_url: Url, index: String, auth: AuthMethod) -> Result<Self> {
        let mut headers = HeaderMap::new();

        match auth {
            AuthMethod::None => {}
            AuthMethod::Basic { user, password } => {
                let credentials = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", user, password));
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Basic {}", credentials))
                        .map_err(|e| ImportError::Other(e.to_string()))?,
                );
            }
            AuthMethod::ApiKey(key) => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("ApiKey {}", key))
                        .map_err(|e| ImportError::Other(e.to_string()))?,
                );
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            client,
            base_url,
            index,
            batch_size: 1000,
        })
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }
}

#[derive(Debug, Deserialize)]
struct ScrollResponse {
    _scroll_id: Option<String>,
    hits: ScrollHits,
}

#[derive(Debug, Deserialize)]
struct ScrollHits {
    hits: Vec<ScrollHit>,
}

#[derive(Debug, Deserialize)]
struct ScrollHit {
    _id: String,
    _source: serde_json::Value,
}

#[async_trait]
impl ImportSource for ElasticsearchSource {
    async fn fetch_schema(&self) -> Result<SourceSchema> {
        let url = self.base_url.join(&format!("{}/_mapping", self.index))?;

        let response = self.client.get(url).send().await?;

        if response.status() == 404 {
            return Err(ImportError::IndexNotFound(self.index.clone()));
        }

        if response.status() == 401 || response.status() == 403 {
            return Err(ImportError::Auth {
                status: response.status().as_u16(),
            });
        }

        let mapping_response: EsMappingResponse = response.json().await?;

        // Get first index from response (handles aliases and patterns)
        let (index_name, index_mapping) = mapping_response
            .indices
            .into_iter()
            .next()
            .ok_or_else(|| ImportError::IndexNotFound(self.index.clone()))?;

        convert_es_mapping(&index_name, &index_mapping.mappings)
    }

    async fn count_documents(&self) -> Result<u64> {
        let url = self.base_url.join(&format!("{}/_count", self.index))?;

        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(ImportError::Other(format!(
                "Count failed with status {}",
                response.status()
            )));
        }

        #[derive(Deserialize)]
        struct CountResponse {
            count: u64,
        }

        let count_response: CountResponse = response.json().await?;
        Ok(count_response.count)
    }

    fn stream_documents(&self) -> Pin<Box<dyn Stream<Item = Result<SourceDocument>> + Send + '_>> {
        Box::pin(async_stream::try_stream! {
            // Initialize scroll
            let url = self.base_url.join(&format!(
                "{}/_search?scroll=5m&size={}",
                self.index, self.batch_size
            ))?;

            let response = self.client
                .post(url)
                .json(&serde_json::json!({
                    "query": { "match_all": {} }
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                Err(ImportError::Other(format!(
                    "Scroll init failed: {}",
                    response.status()
                )))?;
            }

            let mut scroll_response: ScrollResponse = response.json().await?;
            let mut scroll_id = scroll_response._scroll_id.clone();

            // Yield first batch
            for hit in scroll_response.hits.hits {
                yield SourceDocument {
                    id: hit._id,
                    fields: hit._source,
                };
            }

            // Continue scrolling
            while let Some(sid) = scroll_id.take() {
                let url = self.base_url.join("_search/scroll")?;

                let response = self.client
                    .post(url)
                    .json(&serde_json::json!({
                        "scroll": "5m",
                        "scroll_id": sid
                    }))
                    .send()
                    .await?;

                if !response.status().is_success() {
                    break;
                }

                scroll_response = response.json().await?;

                if scroll_response.hits.hits.is_empty() {
                    // Clear scroll
                    if let Some(final_id) = &scroll_response._scroll_id {
                        let _ = self.client
                            .delete(self.base_url.join("_search/scroll")?)
                            .json(&serde_json::json!({ "scroll_id": final_id }))
                            .send()
                            .await;
                    }
                    break;
                }

                scroll_id = scroll_response._scroll_id.clone();

                for hit in scroll_response.hits.hits {
                    yield SourceDocument {
                        id: hit._id,
                        fields: hit._source,
                    };
                }
            }
        })
    }

    fn source_name(&self) -> &str {
        "elasticsearch"
    }
}
