//! ONNX embedding provider with auto-download support

#[cfg(feature = "provider-onnx")]
mod _inner {
    use crate::embedding::{EmbeddingProvider, Embedder, ModelCache, ModelConfig};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Default models supported with auto-download
    pub const DEFAULT_MODEL: &str = "all-MiniLM-L6-v2";

    /// Model dimension mapping for known models
    fn get_model_dimension(model_id: &str) -> usize {
        match model_id {
            "all-MiniLM-L6-v2" => 384,
            "all-MiniLM-L12-v2" => 384,
            "all-mpnet-base-v2" => 768,
            "paraphrase-MiniLM-L6-v2" => 384,
            "paraphrase-multilingual-MiniLM-L12-v2" => 384,
            "multi-qa-MiniLM-L6-cos-v1" => 384,
            "msmarco-MiniLM-L6-cos-v5" => 384,
            _ => 384, // Default to 384 for unknown models
        }
    }

    /// ONNX embedding provider
    pub struct OnnxProvider {
        embedder: Arc<Mutex<Embedder>>,
        model_name: String,
        dimension: usize,
    }

    impl OnnxProvider {
        /// Create a new ONNX provider with auto-download
        ///
        /// If `model_path` is provided, uses the local model file.
        /// If `model_id` is provided, downloads from HuggingFace if not cached.
        /// If neither is provided, uses the default model (all-MiniLM-L6-v2).
        pub async fn new(
            model_path: Option<String>,
            model_id: Option<String>,
            cache_dir: Option<PathBuf>,
        ) -> Result<Self> {
            let (model_name, config) = if let Some(path) = model_path {
                // Use explicit model path
                let path = PathBuf::from(&path);
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("custom")
                    .to_string();

                let mut config = ModelConfig::new(&name);
                config.cache_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                (name, config)
            } else {
                // Use model_id or default
                let model_name = model_id.unwrap_or_else(|| DEFAULT_MODEL.to_string());
                let dimension = get_model_dimension(&model_name);

                let mut config = ModelConfig::new(&model_name);
                config.dimension = dimension;

                if let Some(dir) = cache_dir {
                    config.cache_dir = dir;
                }

                (model_name, config)
            };

            // Ensure model is downloaded
            tracing::info!("Initializing ONNX provider for model: {}", model_name);
            ModelCache::ensure_model(&config).await?;

            // Create embedder
            let embedder = Embedder::new(config.clone()).await?;
            let dimension = config.dimension;

            Ok(Self {
                embedder: Arc::new(Mutex::new(embedder)),
                model_name,
                dimension,
            })
        }

        /// Create with default model
        pub async fn default_model(cache_dir: Option<PathBuf>) -> Result<Self> {
            Self::new(None, None, cache_dir).await
        }
    }

    #[async_trait]
    impl EmbeddingProvider for OnnxProvider {
        async fn embed(&self, text: &str) -> Result<Vec<f32>> {
            let embedder = self.embedder.lock().await;
            embedder.embed(text)
        }

        async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            let embedder = self.embedder.lock().await;
            embedder.embed_batch(texts)
        }

        fn model_name(&self) -> &str {
            &self.model_name
        }

        fn dimensions(&self) -> usize {
            self.dimension
        }
    }
}

#[cfg(feature = "provider-onnx")]
pub use _inner::OnnxProvider;
