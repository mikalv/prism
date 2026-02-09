//! Embedding provider trait and implementations

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Configuration for embedding providers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProviderConfig {
    /// Ollama local embedding server
    Ollama { url: String, model: String },
    /// OpenAI-compatible API (works with OpenAI, Azure, Together, etc.)
    OpenAI {
        url: String,
        api_key: String,
        model: String,
    },
    /// Local ONNX model with auto-download from HuggingFace
    #[cfg(feature = "provider-onnx")]
    Onnx {
        /// Explicit path to model.onnx file (overrides model_id)
        model_path: Option<String>,
        /// HuggingFace model ID (e.g., "all-MiniLM-L6-v2")
        /// Will be auto-downloaded if not cached
        model_id: Option<String>,
        /// Custom cache directory for models
        cache_dir: Option<String>,
    },
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self::Ollama {
            url: "http://localhost:11434".to_string(),
            model: "nomic-embed-text".to_string(),
        }
    }
}

/// Trait for embedding providers
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;

    /// Generate embeddings for multiple texts (batch)
    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        // Default implementation: call embed() for each text
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// Get the model name (used for cache keys)
    fn model_name(&self) -> &str;

    /// Get the embedding dimensions
    fn dimensions(&self) -> usize;
}

/// Create an embedding provider from configuration
pub async fn create_provider(
    config: &ProviderConfig,
) -> anyhow::Result<Box<dyn EmbeddingProvider>> {
    match config {
        ProviderConfig::Ollama { url, model } => {
            let provider = super::ollama::OllamaProvider::new(url, model).await?;
            Ok(Box::new(provider))
        }
        ProviderConfig::OpenAI {
            url,
            api_key,
            model,
        } => {
            let provider = super::openai::OpenAIProvider::new(url, api_key, model)?;
            Ok(Box::new(provider))
        }
        #[cfg(feature = "provider-onnx")]
        ProviderConfig::Onnx {
            model_path,
            model_id,
            cache_dir,
        } => {
            let cache_path = cache_dir.as_ref().map(std::path::PathBuf::from);
            let provider =
                super::onnx::OnnxProvider::new(model_path.clone(), model_id.clone(), cache_path)
                    .await?;
            Ok(Box::new(provider))
        }
    }
}
