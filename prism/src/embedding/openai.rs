//! OpenAI-compatible embedding provider
//!
//! Works with OpenAI, Azure OpenAI, Together.ai, Groq, and other compatible APIs.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::provider::EmbeddingProvider;

/// OpenAI-compatible embedding provider
pub struct OpenAIProvider {
    client: Client,
    url: String,
    model: String,
    dimensions: usize,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding_format: Option<&'a str>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbeddingData>,
    model: String,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u32,
    total_tokens: u32,
}

/// Known model dimensions (to avoid probe call when possible)
fn known_dimensions(model: &str) -> Option<usize> {
    match model {
        "text-embedding-3-small" => Some(1536),
        "text-embedding-3-large" => Some(3072),
        "text-embedding-ada-002" => Some(1536),
        _ => None,
    }
}

impl OpenAIProvider {
    /// Create a new OpenAI-compatible provider
    pub fn new(url: &str, api_key: &str, model: &str) -> anyhow::Result<Self> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", api_key).parse()?,
        );

        let client = Client::builder().default_headers(headers).build()?;

        let url = url.trim_end_matches('/').to_string();

        // Use known dimensions or default to 1536
        let dimensions = known_dimensions(model).unwrap_or(1536);

        Ok(Self {
            client,
            url,
            model: model.to_string(),
            dimensions,
        })
    }

    /// Create with explicit dimensions (useful for custom models)
    pub fn with_dimensions(
        url: &str,
        api_key: &str,
        model: &str,
        dimensions: usize,
    ) -> anyhow::Result<Self> {
        let mut provider = Self::new(url, api_key, model)?;
        provider.dimensions = dimensions;
        Ok(provider)
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIProvider {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text]).await?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("OpenAI returned no embeddings"))
    }

    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = EmbedRequest {
            model: &self.model,
            input: texts.to_vec(),
            encoding_format: Some("float"),
        };

        let response = self
            .client
            .post(format!("{}/embeddings", self.url))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI embed failed ({}): {}", status, body);
        }

        let embed_response: EmbedResponse = response.json().await?;

        // Sort by index to ensure correct order
        let mut embeddings: Vec<_> = embed_response.data.into_iter().collect();
        embeddings.sort_by_key(|e| e.index);

        Ok(embeddings.into_iter().map(|e| e.embedding).collect())
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_dimensions() {
        assert_eq!(known_dimensions("text-embedding-3-small"), Some(1536));
        assert_eq!(known_dimensions("text-embedding-3-large"), Some(3072));
        assert_eq!(known_dimensions("unknown-model"), None);
    }

    // Integration test - requires OpenAI API key
    #[tokio::test]
    #[ignore = "requires OpenAI API key"]
    async fn test_openai_embed() {
        let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
        let provider = OpenAIProvider::new(
            "https://api.openai.com/v1",
            &api_key,
            "text-embedding-3-small",
        )
        .unwrap();

        let embedding = provider.embed("Hello, world!").await.unwrap();

        assert_eq!(embedding.len(), 1536);
    }
}
