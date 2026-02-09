//! Cross-encoder reranker using ONNX Runtime
//!
//! Uses a cross-encoder model (e.g., ms-marco-MiniLM-L-6-v2) to score
//! (query, document) pairs. Cross-encoders produce a single relevance
//! score per pair, unlike bi-encoders which produce separate embeddings.

use crate::ranking::reranker::Reranker;
use async_trait::async_trait;

// ============================================================================
// Feature-gated ONNX implementation
// ============================================================================

#[cfg(feature = "provider-onnx")]
mod onnx_impl {
    use super::*;
    use crate::embedding::{ModelCache, ModelConfig};
    use anyhow::{anyhow, Result};

    #[cfg(feature = "provider-onnx-real")]
    mod real {
        use super::*;
        use anyhow::Result;
        use ndarray::Array2;
        use ort::session::builder::GraphOptimizationLevel;
        use ort::session::Session;
        use ort::value::Tensor;
        use std::sync::{Arc, Mutex};
        use tokenizers::Tokenizer;

        pub struct CrossEncoderReal {
            session: Arc<Mutex<Session>>,
            tokenizer: Arc<Tokenizer>,
            max_length: usize,
            model_name: String,
        }

        impl CrossEncoderReal {
            pub async fn new(config: &ModelConfig, max_length: usize) -> Result<Self> {
                ModelCache::ensure_model(config).await?;

                tracing::info!(
                    "Loading cross-encoder ONNX model from {:?}",
                    config.model_path()
                );
                let session = Session::builder()?
                    .with_optimization_level(GraphOptimizationLevel::Level3)?
                    .commit_from_file(config.model_path())?;

                tracing::info!(
                    "Loading cross-encoder tokenizer from {:?}",
                    config.tokenizer_path()
                );
                let tokenizer = Tokenizer::from_file(config.tokenizer_path())
                    .map_err(|e| anyhow!("Tokenizer load failed: {}", e))?;

                Ok(Self {
                    session: Arc::new(Mutex::new(session)),
                    tokenizer: Arc::new(tokenizer),
                    max_length,
                    model_name: config.model_name.clone(),
                })
            }

            /// Score (query, document) pairs using the cross-encoder model.
            /// Returns one score per pair.
            pub fn score_pairs(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>> {
                if documents.is_empty() {
                    return Ok(vec![]);
                }

                // Cross-encoders use sentence-pair encoding:
                // tokenizer.encode(query, document) produces [CLS] query [SEP] document [SEP]
                let mut all_input_ids: Vec<Vec<i64>> = Vec::with_capacity(documents.len());
                let mut all_attention_masks: Vec<Vec<i64>> = Vec::with_capacity(documents.len());
                let mut all_token_type_ids: Vec<Vec<i64>> = Vec::with_capacity(documents.len());

                for &doc in documents {
                    let encoding = self
                        .tokenizer
                        .encode((query, doc), true)
                        .map_err(|e| anyhow!("Tokenization failed: {}", e))?;

                    let mut ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
                    let mut mask: Vec<i64> =
                        encoding.get_attention_mask().iter().map(|&m| m as i64).collect();
                    let mut type_ids: Vec<i64> =
                        encoding.get_type_ids().iter().map(|&t| t as i64).collect();

                    // Truncate to max_length
                    ids.truncate(self.max_length);
                    mask.truncate(self.max_length);
                    type_ids.truncate(self.max_length);

                    all_input_ids.push(ids);
                    all_attention_masks.push(mask);
                    all_token_type_ids.push(type_ids);
                }

                let batch_size = documents.len();
                let seq_len = all_input_ids.iter().map(|ids| ids.len()).max().unwrap_or(0);

                // Pad to uniform length
                let mut flat_ids = Vec::with_capacity(batch_size * seq_len);
                let mut flat_mask = Vec::with_capacity(batch_size * seq_len);
                let mut flat_types = Vec::with_capacity(batch_size * seq_len);

                for i in 0..batch_size {
                    let padding = seq_len - all_input_ids[i].len();
                    flat_ids.extend_from_slice(&all_input_ids[i]);
                    flat_ids.extend(std::iter::repeat(0i64).take(padding));

                    flat_mask.extend_from_slice(&all_attention_masks[i]);
                    flat_mask.extend(std::iter::repeat(0i64).take(padding));

                    flat_types.extend_from_slice(&all_token_type_ids[i]);
                    flat_types.extend(std::iter::repeat(0i64).take(padding));
                }

                let input_ids_array = Array2::from_shape_vec((batch_size, seq_len), flat_ids)?;
                let attention_mask_array =
                    Array2::from_shape_vec((batch_size, seq_len), flat_mask)?;
                let token_type_ids_array =
                    Array2::from_shape_vec((batch_size, seq_len), flat_types)?;

                let input_ids_tensor = Tensor::from_array(input_ids_array)?;
                let attention_mask_tensor = Tensor::from_array(attention_mask_array)?;
                let token_type_ids_tensor = Tensor::from_array(token_type_ids_array)?;

                let mut session = self
                    .session
                    .lock()
                    .map_err(|e| anyhow!("Session lock poisoned: {}", e))?;

                let outputs = session.run(ort::inputs![
                    "input_ids" => input_ids_tensor,
                    "attention_mask" => attention_mask_tensor,
                    "token_type_ids" => token_type_ids_tensor,
                ])?;

                // Cross-encoder output: [batch_size, 1] or [batch_size] logits
                let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;

                let scores: Vec<f32> = (0..batch_size)
                    .map(|i| {
                        // The model may output [batch, num_labels] where num_labels=1 for regression
                        data.get(i).copied().unwrap_or(0.0)
                    })
                    .collect();

                Ok(scores)
            }

            pub fn name(&self) -> &str {
                &self.model_name
            }
        }
    }

    /// Cross-encoder reranker using ONNX models
    pub struct CrossEncoderReranker {
        #[cfg(feature = "provider-onnx-real")]
        inner: real::CrossEncoderReal,
        #[cfg(not(feature = "provider-onnx-real"))]
        _model_name: String,
    }

    impl CrossEncoderReranker {
        /// Create a new cross-encoder reranker.
        ///
        /// # Arguments
        /// * `model_name` - HuggingFace model ID (e.g., "cross-encoder/ms-marco-MiniLM-L-6-v2")
        /// * `max_length` - Maximum input sequence length
        pub async fn new(model_name: &str, max_length: usize) -> Result<Self> {
            let config = ModelConfig::new(model_name);

            #[cfg(feature = "provider-onnx-real")]
            {
                let inner = real::CrossEncoderReal::new(&config, max_length).await?;
                Ok(Self { inner })
            }
            #[cfg(not(feature = "provider-onnx-real"))]
            {
                let _ = config;
                let _ = max_length;
                tracing::warn!(
                    "Cross-encoder reranker created without ONNX runtime (provider-onnx-real not enabled). \
                     Reranking will return original scores."
                );
                Ok(Self {
                    _model_name: model_name.to_string(),
                })
            }
        }
    }

    #[async_trait]
    impl Reranker for CrossEncoderReranker {
        async fn rerank(&self, query: &str, documents: &[&str]) -> Result<Vec<f32>> {
            #[cfg(feature = "provider-onnx-real")]
            {
                self.inner.score_pairs(query, documents)
            }
            #[cfg(not(feature = "provider-onnx-real"))]
            {
                let _ = query;
                // Without the real ONNX runtime, return zeros (no reranking effect)
                Ok(documents.iter().map(|_| 0.0).collect())
            }
        }

        fn name(&self) -> &str {
            #[cfg(feature = "provider-onnx-real")]
            {
                self.inner.name()
            }
            #[cfg(not(feature = "provider-onnx-real"))]
            {
                &self._model_name
            }
        }
    }
}

#[cfg(feature = "provider-onnx")]
pub use onnx_impl::CrossEncoderReranker;

// ============================================================================
// Stub when ONNX feature is not enabled
// ============================================================================

#[cfg(not(feature = "provider-onnx"))]
mod stub {
    use super::*;
    use anyhow::Result;

    /// Stub cross-encoder reranker when ONNX is not available
    pub struct CrossEncoderReranker {
        model_name: String,
    }

    impl CrossEncoderReranker {
        pub async fn new(model_name: &str, _max_length: usize) -> Result<Self> {
            tracing::warn!(
                "Cross-encoder reranker '{}' requested but provider-onnx feature not enabled. \
                 Reranking will pass through original scores.",
                model_name
            );
            Ok(Self {
                model_name: model_name.to_string(),
            })
        }
    }

    #[async_trait]
    impl Reranker for CrossEncoderReranker {
        async fn rerank(&self, _query: &str, documents: &[&str]) -> Result<Vec<f32>> {
            Ok(documents.iter().map(|_| 0.0).collect())
        }

        fn name(&self) -> &str {
            &self.model_name
        }
    }
}

#[cfg(not(feature = "provider-onnx"))]
pub use stub::CrossEncoderReranker;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::SearchResult;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_cross_encoder_creation() {
        // Without provider-onnx-real, this should still succeed (stub/no-op mode)
        let reranker = CrossEncoderReranker::new("cross-encoder/ms-marco-MiniLM-L-6-v2", 512)
            .await
            .unwrap();
        assert_eq!(reranker.name(), "cross-encoder/ms-marco-MiniLM-L-6-v2");
    }

    #[tokio::test]
    async fn test_cross_encoder_rerank_stub() {
        let reranker = CrossEncoderReranker::new("test-model", 512)
            .await
            .unwrap();
        let scores = reranker
            .rerank("query text", &["doc one", "doc two", "doc three"])
            .await
            .unwrap();
        assert_eq!(scores.len(), 3);
    }

    #[tokio::test]
    async fn test_cross_encoder_rerank_results_uses_default() {
        let reranker = CrossEncoderReranker::new("test-model", 512)
            .await
            .unwrap();
        let results = vec![
            SearchResult {
                id: "1".to_string(),
                score: 1.0,
                fields: HashMap::from([("title".to_string(), serde_json::json!("first doc"))]),
                highlight: None,
            },
            SearchResult {
                id: "2".to_string(),
                score: 0.5,
                fields: HashMap::from([("title".to_string(), serde_json::json!("second doc"))]),
                highlight: None,
            },
        ];
        let scores = reranker
            .rerank_results("query", &results, &["title".to_string()])
            .await
            .unwrap();
        assert_eq!(scores.len(), 2);
    }
}
