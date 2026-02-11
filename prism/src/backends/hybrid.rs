use crate::backends::r#trait::{
    BackendStats, Document, Query, SearchBackend, SearchResult, SearchResults,
};
use crate::schema::types::{HybridConfig, ScoreNormalization, VectorDistance};
use crate::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// Simple hybrid search coordinator that merges text and vector backend results.
/// Supports two merging strategies: Reciprocal Rank Fusion (RRF) and weighted merge.
/// If one backend has no results, returns the other backend's results.
pub struct HybridSearchCoordinator {
    pub text_backend: Arc<dyn SearchBackend>,
    pub vector_backend: Arc<dyn SearchBackend>,
    /// Weight for vector scores in [0.0, 1.0]
    pub vector_weight: f32,
    /// Default merge strategy: "rrf" or "weighted"
    pub default_strategy: String,
    /// RRF k parameter (default: 60)
    pub rrf_k: usize,
    /// Default text weight for weighted merge
    pub text_weight: f32,
    /// Score normalization strategy for weighted merge
    pub normalization: ScoreNormalization,
    /// Vector distance metric (for metric-aware normalization)
    pub distance_metric: Option<VectorDistance>,
}

impl HybridSearchCoordinator {
    pub fn new(
        text_backend: Arc<dyn SearchBackend>,
        vector_backend: Arc<dyn SearchBackend>,
        vector_weight: f32,
    ) -> Self {
        Self {
            text_backend,
            vector_backend,
            vector_weight,
            default_strategy: "rrf".to_string(),
            rrf_k: 60,
            text_weight: 1.0 - vector_weight,
            normalization: ScoreNormalization::default(),
            distance_metric: None,
        }
    }

    /// Create a new coordinator with schema-level HybridConfig defaults.
    pub fn with_config(
        text_backend: Arc<dyn SearchBackend>,
        vector_backend: Arc<dyn SearchBackend>,
        vector_weight: f32,
        config: &HybridConfig,
        distance_metric: Option<VectorDistance>,
    ) -> Self {
        Self {
            text_backend,
            vector_backend,
            vector_weight,
            default_strategy: config.default_strategy.clone(),
            rrf_k: config.rrf_k,
            text_weight: config.text_weight,
            normalization: config.normalization.clone(),
            distance_metric,
        }
    }

    #[tracing::instrument(name = "merge_results", skip(self, text, vector))]
    async fn merge_results(
        &self,
        text: SearchResults,
        vector: SearchResults,
        limit: usize,
    ) -> SearchResults {
        // Default to weighted merge using the instance's vector_weight
        Self::merge_weighted_with_normalization(
            text,
            vector,
            1.0 - self.vector_weight,
            self.vector_weight,
            limit,
            &self.normalization,
            self.distance_metric.as_ref(),
        )
    }

    /// Public weighted merge helper for testing and reuse.
    /// Uses MaxNorm normalization by default.
    pub fn merge_weighted_public(
        text: SearchResults,
        vector: SearchResults,
        text_weight: f32,
        vector_weight: f32,
        limit: usize,
    ) -> SearchResults {
        Self::merge_weighted_with_normalization(
            text,
            vector,
            text_weight,
            vector_weight,
            limit,
            &ScoreNormalization::MaxNorm,
            None,
        )
    }

    /// Weighted merge with configurable normalization strategy.
    pub fn merge_weighted_with_normalization(
        text: SearchResults,
        vector: SearchResults,
        text_weight: f32,
        vector_weight: f32,
        limit: usize,
        normalization: &ScoreNormalization,
        distance_metric: Option<&VectorDistance>,
    ) -> SearchResults {
        use std::collections::HashMap;

        let mut combined: HashMap<String, SearchResult> = HashMap::new();

        // Compute max scores for normalization
        let text_max = text
            .results
            .iter()
            .map(|r| r.score)
            .fold(f32::NAN, f32::max);
        let vec_max = vector
            .results
            .iter()
            .map(|r| r.score)
            .fold(f32::NAN, f32::max);

        for r in text.results {
            let norm = match normalization {
                ScoreNormalization::None => r.score,
                ScoreNormalization::MaxNorm | ScoreNormalization::MetricAware => {
                    // BM25 scores are unbounded positive — always divide by max
                    if text_max.is_nan() || text_max == 0.0 {
                        r.score
                    } else {
                        r.score / text_max
                    }
                }
            };
            combined.insert(
                r.id.clone(),
                SearchResult {
                    id: r.id.clone(),
                    score: text_weight * norm,
                    fields: r.fields,
                    highlight: r.highlight,
                },
            );
        }

        for r in vector.results {
            let norm = match normalization {
                ScoreNormalization::None => r.score,
                ScoreNormalization::MaxNorm => {
                    if vec_max.is_nan() || vec_max == 0.0 {
                        r.score
                    } else {
                        r.score / vec_max
                    }
                }
                ScoreNormalization::MetricAware => {
                    // Metric-aware: behavior depends on distance metric
                    match distance_metric {
                        Some(VectorDistance::Cosine) => {
                            // Cosine similarity scores are in [0, 1], use as-is
                            r.score
                        }
                        Some(VectorDistance::Euclidean) => {
                            // 1 - L2_dist can go negative; clamp then divide by max
                            let clamped = r.score.max(0.0);
                            if vec_max.is_nan() || vec_max <= 0.0 {
                                clamped
                            } else {
                                clamped / vec_max
                            }
                        }
                        Some(VectorDistance::Dot) | None => {
                            // Dot product is unbounded; divide by max
                            if vec_max.is_nan() || vec_max == 0.0 {
                                r.score
                            } else {
                                r.score / vec_max
                            }
                        }
                    }
                }
            };
            combined
                .entry(r.id.clone())
                .and_modify(|e| {
                    e.score += vector_weight * norm;
                })
                .or_insert(SearchResult {
                    id: r.id.clone(),
                    score: vector_weight * norm,
                    fields: r.fields,
                    highlight: r.highlight,
                });
        }

        let mut out: Vec<SearchResult> = combined.into_values().collect();
        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total = out.len();
        out.truncate(limit);

        SearchResults {
            results: out,
            total,
            latency_ms: 0,
        }
    }

    /// Public RRF merge helper for testing and reuse.
    /// k is the RRF constant (typical values 60-100). Higher k reduces rank influence.
    pub fn merge_rrf_public(
        text: SearchResults,
        vector: SearchResults,
        k: usize,
        limit: usize,
    ) -> SearchResults {
        use std::collections::HashMap;

        let mut scores: HashMap<String, f32> = HashMap::new();
        let mut fields_map: HashMap<String, std::collections::HashMap<String, serde_json::Value>> =
            HashMap::new();

        // Process text results
        for (i, r) in text.results.into_iter().enumerate() {
            let rank = i + 1; // ranks start at 1
            let contrib = 1.0_f32 / ((k as f32) + (rank as f32));
            *scores.entry(r.id.clone()).or_insert(0.0) += contrib;
            fields_map.entry(r.id.clone()).or_insert(r.fields);
        }

        // Process vector results
        for (i, r) in vector.results.into_iter().enumerate() {
            let rank = i + 1;
            let contrib = 1.0_f32 / ((k as f32) + (rank as f32));
            *scores.entry(r.id.clone()).or_insert(0.0) += contrib;
            fields_map.entry(r.id.clone()).or_insert(r.fields);
        }

        let mut out: Vec<SearchResult> = scores
            .into_iter()
            .map(|(id, score)| {
                let fields = fields_map.remove(&id).unwrap_or_default();
                SearchResult {
                    id,
                    score,
                    fields,
                    highlight: None,
                }
            })
            .collect();

        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total = out.len();
        out.truncate(limit);

        SearchResults {
            results: out,
            total,
            latency_ms: 0,
        }
    }
}

#[async_trait]
impl SearchBackend for HybridSearchCoordinator {
    async fn index(&self, collection: &str, docs: Vec<Document>) -> Result<()> {
        // Index into both backends
        self.text_backend.index(collection, docs.clone()).await?;
        self.vector_backend.index(collection, docs).await?;
        Ok(())
    }

    async fn search(&self, collection: &str, query: Query) -> Result<SearchResults> {
        // Attempt to parse a vector from query_string; if present, run vector and text searches accordingly
        let maybe_vec: Option<Vec<f32>> = serde_json::from_str(&query.query_string).ok();

        let (tres, vres) = if let Some(vec) = maybe_vec {
            // If query_string is a vector, run vector search and run a text search with the provided fields but empty string
            let vec_q = Query {
                query_string: serde_json::to_string(&vec).unwrap(),
                fields: vec![],
                limit: query.limit,
                offset: query.offset,
                merge_strategy: None,
                text_weight: None,
                vector_weight: None,
                highlight: None,
                rrf_k: None,
                min_score: None,
                score_function: None,
                skip_ranking: true, // Skip ranking in sub-queries; apply after merge
            };
            let text_q = Query {
                query_string: "".to_string(),
                fields: query.fields.clone(),
                limit: query.limit,
                offset: query.offset,
                merge_strategy: None,
                text_weight: None,
                vector_weight: None,
                highlight: query.highlight.clone(),
                rrf_k: None,
                min_score: None,
                score_function: None,
                skip_ranking: true,
            };
            let t = self.text_backend.search(collection, text_q);
            let v = self.vector_backend.search(collection, vec_q);
            tokio::join!(t, v)
        } else {
            // No vector provided: run only text search
            let text_q = Query {
                query_string: query.query_string.clone(),
                fields: query.fields.clone(),
                limit: query.limit,
                offset: query.offset,
                merge_strategy: query.merge_strategy.clone(),
                text_weight: query.text_weight,
                vector_weight: query.vector_weight,
                highlight: query.highlight.clone(),
                rrf_k: query.rrf_k,
                min_score: query.min_score,
                score_function: query.score_function.clone(),
                skip_ranking: query.skip_ranking,
            };
            let t = self.text_backend.search(collection, text_q).await?;
            return Ok(t);
        };

        let tres = tres?;
        let vres = vres?;

        // Decide merge strategy: per-query override > schema default
        let strategy = query
            .merge_strategy
            .as_deref()
            .unwrap_or(&self.default_strategy);

        let merged = match strategy {
            "weighted" => {
                let text_w = query.text_weight.unwrap_or(self.text_weight);
                let vector_w = query.vector_weight.unwrap_or(self.vector_weight);
                Self::merge_weighted_with_normalization(
                    tres,
                    vres,
                    text_w,
                    vector_w,
                    query.limit,
                    &self.normalization,
                    self.distance_metric.as_ref(),
                )
            }
            _ => {
                // RRF: per-query rrf_k > schema default
                let k = query.rrf_k.unwrap_or(self.rrf_k);
                Self::merge_rrf_public(tres, vres, k, query.limit)
            }
        };

        Ok(merged)
    }

    async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>> {
        // Prefer text backend for metadata
        if let Some(d) = self.text_backend.get(collection, id).await? {
            return Ok(Some(d));
        }
        self.vector_backend.get(collection, id).await
    }

    async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<()> {
        self.text_backend.delete(collection, ids.clone()).await?;
        self.vector_backend.delete(collection, ids).await?;
        Ok(())
    }

    async fn stats(&self, collection: &str) -> Result<BackendStats> {
        // Combine stats conservatively (max document_count)
        let t = self.text_backend.stats(collection).await?;
        let v = self.vector_backend.stats(collection).await?;
        Ok(BackendStats {
            document_count: std::cmp::max(t.document_count, v.document_count),
            size_bytes: t.size_bytes + v.size_bytes,
        })
    }

    async fn search_with_aggs(
        &self,
        _collection: &str,
        _query: &Query,
        _aggregations: Vec<crate::aggregations::AggregationRequest>,
    ) -> Result<crate::backends::SearchResultsWithAggs> {
        Err(crate::error::Error::NotImplemented(
            "Aggregations not supported for hybrid backend".to_string(),
        ))
    }
}
