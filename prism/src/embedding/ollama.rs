//! Ollama embedding provider

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::provider::EmbeddingProvider;

/// Ollama embedding provider
pub struct OllamaProvider {
    client: Client,
    url: String,
    model: String,
    dimensions: usize,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Serialize)]
struct EmbedBatchRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl OllamaProvider {
    /// Create a new Ollama provider
    pub async fn new(url: &str, model: &str) -> anyhow::Result<Self> {
        let client = Client::new();
        let url = url.trim_end_matches('/').to_string();

        // Get dimensions by doing a test embedding
        let dimensions = Self::probe_dimensions(&client, &url, model).await?;

        Ok(Self {
            client,
            url,
            model: model.to_string(),
            dimensions,
        })
    }

    async fn probe_dimensions(client: &Client, url: &str, model: &str) -> anyhow::Result<usize> {
        let request = EmbedRequest {
            model,
            input: "test",
        };

        let response = client
            .post(format!("{}/api/embed", url))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama probe failed ({}): {}", status, body);
        }

        let embed_response: EmbedResponse = response.json().await?;

        if embed_response.embeddings.is_empty() {
            anyhow::bail!("Ollama returned empty embeddings");
        }

        Ok(embed_response.embeddings[0].len())
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaProvider {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let request = EmbedRequest {
            model: &self.model,
            input: text,
        };

        let response = self
            .client
            .post(format!("{}/api/embed", self.url))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama embed failed ({}): {}", status, body);
        }

        let embed_response: EmbedResponse = response.json().await?;

        embed_response
            .embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Ollama returned no embeddings"))
    }

    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = EmbedBatchRequest {
            model: &self.model,
            input: texts.to_vec(),
        };

        let response = self
            .client
            .post(format!("{}/api/embed", self.url))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Ollama batch embed failed ({}): {}", status, body);
        }

        let embed_response: EmbedResponse = response.json().await?;

        if embed_response.embeddings.len() != texts.len() {
            anyhow::bail!(
                "Ollama returned {} embeddings for {} texts",
                embed_response.embeddings.len(),
                texts.len()
            );
        }

        Ok(embed_response.embeddings)
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

    // Integration test - requires running Ollama with nomic-embed-text
    #[tokio::test]
    #[ignore = "requires running Ollama server"]
    async fn test_ollama_embed() {
        let provider = OllamaProvider::new("http://localhost:11434", "nomic-embed-text")
            .await
            .unwrap();

        let embedding = provider.embed("Hello, world!").await.unwrap();

        assert!(!embedding.is_empty());
        assert_eq!(embedding.len(), provider.dimensions());

        // Check embedding is normalized (roughly)
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 0.1,
            "Embedding should be roughly normalized"
        );
    }

    #[tokio::test]
    #[ignore = "requires running Ollama server"]
    async fn test_ollama_batch_embed() {
        let provider = OllamaProvider::new("http://localhost:11434", "nomic-embed-text")
            .await
            .unwrap();

        let texts = vec!["Hello", "World", "Test"];
        let embeddings = provider.embed_batch(&texts).await.unwrap();

        assert_eq!(embeddings.len(), 3);
        for emb in &embeddings {
            assert_eq!(emb.len(), provider.dimensions());
        }
    }
}
