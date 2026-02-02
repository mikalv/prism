// Model download and caching

#[cfg(feature = "provider-onnx")]
mod _inner {
    use anyhow::{Context, Result};
    use std::path::{Path, PathBuf};
    use std::fs;

    /// Model configuration
    #[derive(Debug, Clone)]
    pub struct ModelConfig {
        pub model_name: String,
        pub cache_dir: PathBuf,
        pub dimension: usize,
    }

    impl ModelConfig {
        /// Create new model config with defaults
        pub fn new(model_name: impl Into<String>) -> Self {
            let cache_dir = dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("engraph")
                .join("models");
            
            Self {
                model_name: model_name.into(),
                cache_dir,
                dimension: 384,
            }
        }
        
        /// Get model directory path
        pub fn model_dir(&self) -> PathBuf {
            self.cache_dir.join(&self.model_name)
        }
        
        /// Get model.onnx path
        pub fn model_path(&self) -> PathBuf {
            self.model_dir().join("model.onnx")
        }
        
        /// Get tokenizer.json path
        pub fn tokenizer_path(&self) -> PathBuf {
            self.model_dir().join("tokenizer.json")
        }
        
        /// Check if model is cached
        pub fn is_cached(&self) -> bool {
            self.model_path().exists() && self.tokenizer_path().exists()
        }
    }

    /// Model cache manager
    pub struct ModelCache;

    impl ModelCache {
        /// Ensure model is available, download if needed
        pub async fn ensure_model(config: &ModelConfig) -> Result<()> {
            if config.is_cached() {
                tracing::info!("Model {} found in cache", config.model_name);
                return Ok(());
            }
            
            tracing::info!("Model {} not cached, downloading...", config.model_name);
            Self::download_model(config).await?;
            
            Ok(())
        }
        
        /// Download model from HuggingFace
        async fn download_model(config: &ModelConfig) -> Result<()> {
            // Create cache directory
            fs::create_dir_all(&config.model_dir())
                .context("Failed to create cache directory")?;
            
            let base_url = format!(
                "https://huggingface.co/sentence-transformers/{}/resolve/main",
                config.model_name
            );
            
            // Download model.onnx
            tracing::info!("Downloading model.onnx...");
            let model_url = format!("{}/onnx/model.onnx", base_url);
            Self::download_file(&model_url, &config.model_path()).await
                .context("Failed to download model.onnx")?;
            
            // Download tokenizer.json
            tracing::info!("Downloading tokenizer.json...");
            let tokenizer_url = format!("{}/tokenizer.json", base_url);
            Self::download_file(&tokenizer_url, &config.tokenizer_path()).await
                .context("Failed to download tokenizer.json")?;
            
            tracing::info!("Model {} downloaded successfully", config.model_name);
            Ok(())
        }
        
        /// Download file from URL to path
        async fn download_file(url: &str, path: &Path) -> Result<()> {
            let response = reqwest::get(url)
                .await
                .context("Failed to send request")?;
            
            if !response.status().is_success() {
                anyhow::bail!("Download failed with status: {}", response.status());
            }
            
            let bytes = response.bytes()
                .await
                .context("Failed to read response body")?;
            
            fs::write(path, &bytes)
                .context("Failed to write file")?;
            
            Ok(())
        }
    }

}

#[cfg(feature = "provider-onnx")]
pub use _inner::{ModelCache, ModelConfig};
