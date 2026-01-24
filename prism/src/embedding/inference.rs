// Real ONNX embedder + deterministic fallback (feature-gated)

#[cfg(feature = "embedding-gen")]
mod _inner {
    use anyhow::Result;
    use crate::embedding::ModelConfig;

    // Real embedder with ONNX Runtime (ort 2.x API)
    mod real {
        use anyhow::{Result, anyhow};
        use super::ModelConfig;
        use crate::embedding::ModelCache;

        #[cfg(feature = "embedding-gen-real")]
        mod full {
            use anyhow::{Result, anyhow};
            use ort::session::Session;
            use ort::session::builder::GraphOptimizationLevel;
            use ort::value::Tensor;
            use ndarray::Array2;
            use tokenizers::Tokenizer;
            use std::sync::{Arc, Mutex};
            use super::ModelConfig;
            use crate::embedding::ModelCache;

            pub struct RealEmbedderFull {
                session: Arc<Mutex<Session>>,
                tokenizer: Arc<Tokenizer>,
                dimension: usize,
            }

            impl RealEmbedderFull {
                pub async fn new(config: &ModelConfig) -> Result<Self> {
                    ModelCache::ensure_model(config).await?;

                    tracing::info!("Loading ONNX model from {:?}", config.model_path());
                    let session = Session::builder()?
                        .with_optimization_level(GraphOptimizationLevel::Level3)?
                        .commit_from_file(config.model_path())?;

                    tracing::info!("Loading tokenizer from {:?}", config.tokenizer_path());
                    let tokenizer = Tokenizer::from_file(config.tokenizer_path())
                        .map_err(|e| anyhow!("Tokenizer load failed: {}", e))?;

                    Ok(Self {
                        session: Arc::new(Mutex::new(session)),
                        tokenizer: Arc::new(tokenizer),
                        dimension: config.dimension
                    })
                }

                pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
                    if texts.is_empty() { return Ok(vec![]); }

                    // Tokenize with padding
                    let encodings = self.tokenizer
                        .encode_batch(texts.to_vec(), true)
                        .map_err(|e| anyhow!("Tokenization failed: {}", e))?;

                    let batch_size = texts.len();
                    let seq_len = encodings.iter().map(|e| e.len()).max().unwrap_or(0);

                    // Build input tensors
                    let mut input_ids: Vec<i64> = Vec::with_capacity(batch_size * seq_len);
                    let mut attention_mask: Vec<i64> = Vec::with_capacity(batch_size * seq_len);
                    let mut token_type_ids: Vec<i64> = Vec::with_capacity(batch_size * seq_len);

                    for enc in &encodings {
                        let ids = enc.get_ids();
                        let mask = enc.get_attention_mask();
                        let type_ids = enc.get_type_ids();
                        let padding = seq_len - ids.len();

                        input_ids.extend(ids.iter().map(|&id| id as i64));
                        input_ids.extend(std::iter::repeat(0i64).take(padding));

                        attention_mask.extend(mask.iter().map(|&m| m as i64));
                        attention_mask.extend(std::iter::repeat(0i64).take(padding));

                        token_type_ids.extend(type_ids.iter().map(|&t| t as i64));
                        token_type_ids.extend(std::iter::repeat(0i64).take(padding));
                    }

                    // Create ort Tensor values from arrays
                    let input_ids_array = Array2::from_shape_vec(
                        (batch_size, seq_len),
                        input_ids
                    )?;
                    let attention_mask_array = Array2::from_shape_vec(
                        (batch_size, seq_len),
                        attention_mask
                    )?;
                    let token_type_ids_array = Array2::from_shape_vec(
                        (batch_size, seq_len),
                        token_type_ids
                    )?;

                    let input_ids_tensor = Tensor::from_array(input_ids_array)?;
                    let attention_mask_tensor = Tensor::from_array(attention_mask_array)?;
                    let token_type_ids_tensor = Tensor::from_array(token_type_ids_array)?;

                    // Run inference - ort 2.x API
                    let mut session = self.session.lock()
                        .map_err(|e| anyhow!("Session lock poisoned: {}", e))?;
                    let outputs = session.run(ort::inputs![
                        "input_ids" => input_ids_tensor,
                        "attention_mask" => attention_mask_tensor,
                        "token_type_ids" => token_type_ids_tensor,
                    ])?;

                    // Extract embeddings - shape: [batch_size, seq_len, hidden_size]
                    let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
                    let dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();

                    let mut results = Vec::with_capacity(batch_size);

                    if dims.len() == 3 {
                        // [batch, seq_len, hidden] - need mean pooling
                        let seq_dim = dims[1];
                        let hidden_size = dims[2];

                        for b in 0..batch_size {
                            // Mean pooling over sequence dimension
                            let mut embedding = vec![0f32; hidden_size];
                            let valid_tokens = encodings[b].get_attention_mask()
                                .iter().filter(|&&m| m == 1).count();

                            if valid_tokens > 0 {
                                for s in 0..valid_tokens {
                                    let base = b * seq_dim * hidden_size + s * hidden_size;
                                    for d in 0..hidden_size {
                                        embedding[d] += data[base + d];
                                    }
                                }
                                for v in &mut embedding { *v /= valid_tokens as f32; }
                            }

                            // L2 normalize
                            let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
                            if norm > 0.0 { for v in &mut embedding { *v /= norm; } }

                            results.push(embedding);
                        }
                    } else if dims.len() == 2 {
                        // [batch, hidden] - already pooled
                        let hidden_size = dims[1];

                        for b in 0..batch_size {
                            let base = b * hidden_size;
                            let mut embedding: Vec<f32> = data[base..base+hidden_size].to_vec();

                            // L2 normalize
                            let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
                            if norm > 0.0 { for v in &mut embedding { *v /= norm; } }

                            results.push(embedding);
                        }
                    } else {
                        return Err(anyhow!("Unexpected output shape: {:?}", dims));
                    }

                    Ok(results)
                }
            }
        }

        #[cfg(not(feature = "embedding-gen-real"))]
        pub struct RealEmbedder {
            pub dimension: usize,
        }

        #[cfg(not(feature = "embedding-gen-real"))]
        impl RealEmbedder {
            pub async fn new(_config: &ModelConfig) -> Result<Self> {
                Err(anyhow!("Real embedder not enabled; compile with --features embedding-gen-real"))
            }
            pub fn embed_batch(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>> {
                Err(anyhow!("Real embedder not enabled"))
            }
        }

        #[cfg(feature = "embedding-gen-real")]
        pub use full::RealEmbedderFull as RealEmbedder;
    }


    // Deterministic fallback
    mod fallback {
        use sha2::{Digest, Sha256};

        pub struct DeterministicEmbedder {
            dimension: usize,
        }

        impl DeterministicEmbedder {
            pub fn new(dimension: usize) -> Self {
                Self { dimension }
            }

            pub fn embed_batch(&self, texts: &[&str]) -> Vec<Vec<f32>> {
                let mut out = Vec::with_capacity(texts.len());
                for t in texts {
                    let tokens: Vec<&str> = t.split_whitespace().collect();
                    let mut accum = vec![0f32; self.dimension];
                    for (ti, token) in tokens.iter().enumerate() {
                        let mut hasher = Sha256::new();
                        hasher.update(token.as_bytes());
                        let hash = hasher.finalize();
                        for i in 0..self.dimension {
                            let idx = i % hash.len();
                            let val = (hash[idx] as f32) / 255.0;
                            accum[i] += val;
                        }
                        for v in &mut accum { *v *= 1.0 + (ti as f32) * 0.0001; }
                    }
                    if tokens.is_empty() {
                        let mut hasher = Sha256::new();
                        hasher.update(t.as_bytes());
                        let hash = hasher.finalize();
                        for i in 0..self.dimension {
                            let idx = i % hash.len();
                            accum[i] = (hash[idx] as f32) / 255.0;
                        }
                    }
                    let norm: f32 = accum.iter().map(|x| x * x).sum::<f32>().sqrt();
                    if norm > 0.0 { for v in &mut accum { *v /= norm; } }
                    out.push(accum);
                }
                out
            }
        }
    }

    use real::RealEmbedder;
    use fallback::DeterministicEmbedder;

    /// Unified Embedder type that prefers real ONNX but falls back to deterministic
    pub enum Embedder {
        Real(RealEmbedder),
        Fallback(DeterministicEmbedder),
    }

    impl Embedder {
        pub async fn new(config: ModelConfig) -> Result<Self> {
            // Try to construct real embedder
            match RealEmbedder::new(&config).await {
                Ok(re) => Ok(Embedder::Real(re)),
                Err(e) => {
                    tracing::warn!("Real embedder construction failed, falling back: {}", e);
                    Ok(Embedder::Fallback(DeterministicEmbedder::new(config.dimension)))
                }
            }
        }

        pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
            match self {
                Embedder::Real(r) => r.embed_batch(&[text]).map(|v| v.into_iter().next().unwrap()),
                Embedder::Fallback(f) => Ok(f.embed_batch(&[text]).into_iter().next().unwrap()),
            }
        }

        pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            match self {
                Embedder::Real(r) => r.embed_batch(texts),
                Embedder::Fallback(f) => Ok(f.embed_batch(texts)),
            }
        }
    }

}

#[cfg(feature = "embedding-gen")]
pub use _inner::Embedder;
