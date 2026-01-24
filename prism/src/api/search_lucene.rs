use axum::{extract::State, http::StatusCode, Json};
use chrono::{DateTime, Utc};
use crate::query::{
    aggregations::{facets::compute_facets, AggregationRequest, AggregationType},
    engine::boosting::{apply_boost, calculate_context_boost, calculate_recency_decay, DecayFunction},
    parser::LuceneParser,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::collection::manager::CollectionManager;

#[derive(Debug, Deserialize)]
pub struct LuceneSearchRequest {
    pub collection: String,
    pub query: String,

    #[serde(default = "default_limit")]
    pub limit: usize,

    #[serde(default)]
    pub offset: usize,

    #[serde(default)]
    pub facets: Vec<FacetRequest>,

    #[serde(default)]
    pub context: SearchContext,

    /// Enable semantic/vector search (auto-embed query)
    #[serde(default)]
    pub enable_semantic: bool,

    /// Explicit vector query (overrides enable_semantic)
    #[serde(default)]
    pub vector: Option<Vec<f32>>,

    /// Boosting configuration
    #[serde(default)]
    pub boosting: Option<BoostingRequest>,

    /// Merge strategy for hybrid search: "rrf", "weighted"
    #[serde(default)]
    pub merge_strategy: Option<String>,

    /// Text weight for hybrid merge (default: 0.5)
    #[serde(default)]
    pub text_weight: Option<f32>,

    /// Vector weight for hybrid merge (default: 0.5)
    #[serde(default)]
    pub vector_weight: Option<f32>,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Deserialize)]
pub struct FacetRequest {
    pub field: String,
    /// "terms" or "date_histogram"
    pub agg_type: String,
    #[serde(default = "default_facet_size")]
    pub size: usize,
    /// For date_histogram: "hour", "day", "week", "month", "year"
    #[serde(default)]
    pub interval: Option<String>,
}

fn default_facet_size() -> usize {
    10
}

#[derive(Debug, Deserialize)]
pub struct BoostingRequest {
    #[serde(default)]
    pub recency_enabled: bool,
    #[serde(default = "default_recency_field")]
    pub recency_field: String,
    #[serde(default = "default_decay_days")]
    pub recency_decay_days: f64,
    #[serde(default = "default_context_boost")]
    pub context_boost: f32,
}

fn default_recency_field() -> String {
    "timestamp".to_string()
}

fn default_decay_days() -> f64 {
    30.0
}

fn default_context_boost() -> f32 {
    1.5
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SearchContext {
    pub project_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LuceneSearchResponse {
    pub results: Vec<SearchResultItem>,
    pub total: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub facets: Option<HashMap<String, FacetResult>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestions: Option<Vec<String>>,

    pub meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct FacetResult {
    pub buckets: Vec<FacetBucket>,
}

#[derive(Debug, Serialize)]
pub struct FacetBucket {
    pub key: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ResponseMeta {
    pub query_time_ms: f64,
    pub parse_time_ms: f64,
    pub search_time_ms: f64,
    pub query_type: String,
    pub result_count: usize,
    pub total_matches: usize,
}

/// POST /search/lucene
pub async fn search_lucene(
    State(manager): State<Arc<CollectionManager>>,
    Json(req): Json<LuceneSearchRequest>,
) -> Result<Json<LuceneSearchResponse>, StatusCode> {
    let start = std::time::Instant::now();

    // 1. Parse the query using engraph-query
    let ast = match LuceneParser::parse(&req.query) {
        Ok(ast) => ast,
        Err(e) => {
            tracing::warn!(query = %req.query, error = %e, "Query parse failed");
            return Err(StatusCode::BAD_REQUEST);
        }
    };
    let parse_ms = start.elapsed().as_secs_f64() * 1000.0;

    // 2. Execute search with merge strategy parameters
    let query = crate::backends::Query {
        query_string: req.query.clone(),
        fields: vec![],
        limit: req.limit,
        offset: req.offset,
        merge_strategy: req.merge_strategy.clone(),
        text_weight: req.text_weight,
        vector_weight: req.vector_weight,
    };

    let search_results = match manager.search(&req.collection, query).await {
        Ok(results) => results,
        Err(crate::Error::CollectionNotFound(_)) => {
            tracing::warn!(collection = %req.collection, "Collection not found during search");
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!(error = %e, collection = %req.collection, "Search execution failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let search_ms = start.elapsed().as_secs_f64() * 1000.0 - parse_ms;

    // 3. Convert results to mutable vec for boosting
    let mut results: Vec<SearchResultItem> = search_results
        .results
        .into_iter()
        .map(|r| SearchResultItem {
            id: r.id,
            score: r.score,
            fields: r.fields,
        })
        .collect();

    // 4. Apply boosting post-processing
    if let Some(ref boost_config) = req.boosting {
        let now = Utc::now();
        let search_context = extract_search_context(&req.context);

        for result in &mut results {
            let mut recency_mult = 1.0;
            let mut context_mult = 1.0;

            // Recency boost
            if boost_config.recency_enabled {
                if let Some(ts_value) = result.fields.get(&boost_config.recency_field) {
                    if let Some(ts_str) = ts_value.as_str() {
                        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
                            recency_mult = calculate_recency_decay(
                                dt.with_timezone(&Utc),
                                now,
                                DecayFunction::Exponential,
                                chrono::Duration::days(boost_config.recency_decay_days as i64),
                                chrono::Duration::days(1),
                                0.5,
                            );
                        }
                    }
                }
            }

            // Context boost
            if boost_config.context_boost > 1.0 {
                let doc_context = extract_context_from_fields(&result.fields);
                context_mult = calculate_context_boost(
                    &doc_context,
                    &search_context,
                    boost_config.context_boost,
                );
            }

            result.score = apply_boost(result.score, recency_mult, context_mult, 1.0);
        }

        // Re-sort by boosted score
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    }

    // 5. Compute facets from results
    let facets = if !req.facets.is_empty() {
        let result_docs: Vec<HashMap<String, serde_json::Value>> =
            results.iter().map(|r| r.fields.clone()).collect();

        let agg_requests: Vec<AggregationRequest> = req
            .facets
            .iter()
            .map(|f| AggregationRequest {
                field: f.field.clone(),
                agg_type: match f.agg_type.as_str() {
                    "date_histogram" => AggregationType::DateHistogram,
                    "range" => AggregationType::Range,
                    "stats" => AggregationType::Stats,
                    _ => AggregationType::Terms,
                },
                size: f.size,
                interval: f.interval.clone(),
            })
            .collect();

        match compute_facets(agg_requests, &result_docs) {
            Ok(agg_results) => {
                let mut facet_map = HashMap::new();
                for agg in agg_results {
                    facet_map.insert(
                        agg.field,
                        FacetResult {
                            buckets: agg
                                .buckets
                                .into_iter()
                                .map(|b| FacetBucket {
                                    key: b.key,
                                    count: b.count,
                                })
                                .collect(),
                        },
                    );
                }
                Some(facet_map)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Facet computation failed");
                None
            }
        }
    } else {
        None
    };

    let total_ms = start.elapsed().as_secs_f64() * 1000.0;

    // 6. Build response
    let response = LuceneSearchResponse {
        total: search_results.total,
        results,
        facets,
        suggestions: None,
        meta: ResponseMeta {
            query_time_ms: total_ms,
            parse_time_ms: parse_ms,
            search_time_ms: search_ms,
            query_type: ast.query_type().to_string(),
            result_count: search_results.total,
            total_matches: search_results.total,
        },
    };

    tracing::info!(
        collection = %req.collection,
        query = %req.query,
        query_type = %response.meta.query_type,
        results = response.total,
        time_ms = total_ms,
        "Lucene search completed"
    );

    Ok(Json(response))
}

/// Extract search context from request context for boosting
fn extract_search_context(ctx: &SearchContext) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(ref pid) = ctx.project_id {
        map.insert("project_id".to_string(), pid.clone());
    }
    if let Some(ref sid) = ctx.session_id {
        map.insert("session_id".to_string(), sid.clone());
    }
    map
}

/// Extract context fields from document for boosting comparison
fn extract_context_from_fields(fields: &HashMap<String, serde_json::Value>) -> HashMap<String, String> {
    let mut context = HashMap::new();
    for key in &["project_id", "session_id", "file_path"] {
        if let Some(value) = fields.get(*key) {
            if let Some(s) = value.as_str() {
                context.insert(key.to_string(), s.to_string());
            }
        }
    }
    context
}
