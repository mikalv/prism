use crate::api::server::AppState;
use crate::backends::{Document, HighlightConfig, Query, SearchResult, SearchResults};
use crate::collection::CollectionManager;
use crate::ranking::reranker::{RerankOptions, RerankRequest};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct SearchRequest {
    /// Free-form text query. If `vector` is provided, this field may be empty.
    #[serde(default)]
    pub query: Option<String>,
    /// Optional explicit vector query (preferred for hybrid). If present, will be used by HybridSearchCoordinator.
    #[serde(default)]
    pub vector: Option<Vec<f32>>,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub merge_strategy: Option<String>,
    #[serde(default)]
    pub text_weight: Option<f32>,
    #[serde(default)]
    pub vector_weight: Option<f32>,
    /// Optional highlight configuration
    #[serde(default)]
    pub highlight: Option<HighlightConfig>,
    /// Optional reranking override for this request
    #[serde(default)]
    pub rerank: Option<RerankRequest>,
}

fn default_limit() -> usize {
    10
}

#[derive(Deserialize)]
pub struct SimpleSearchRequest {
    pub query: String,
    #[serde(default = "default_simple_limit")]
    pub limit: usize,
}

fn default_simple_limit() -> usize {
    10
}

#[derive(Serialize)]
pub struct SimpleSearchResult {
    pub id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub snippet: Option<String>,
    pub score: f32,
}

#[derive(Serialize)]
pub struct SimpleSearchResponse {
    pub results: Vec<SimpleSearchResult>,
    pub total: usize,
}

fn result_to_simple(result: SearchResult) -> SimpleSearchResult {
    let title = result
        .fields
        .get("title")
        .and_then(|v| v.as_str())
        .map(String::from);
    let url = result
        .fields
        .get("url")
        .or_else(|| result.fields.get("link"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let snippet = result
        .fields
        .get("snippet")
        .or_else(|| result.fields.get("content"))
        .or_else(|| result.fields.get("description"))
        .and_then(|v| v.as_str())
        .map(String::from);

    SimpleSearchResult {
        id: result.id,
        title,
        url,
        snippet,
        score: result.score,
    }
}

#[tracing::instrument(
    name = "search",
    skip(manager, request),
    fields(collection = %collection, search_type = "text")
)]
pub async fn search(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResults>, StatusCode> {
    let start = std::time::Instant::now();

    let qstr = if let Some(vec) = request.vector.clone() {
        serde_json::to_string(&vec).unwrap_or_default()
    } else {
        request.query.clone().unwrap_or_default()
    };

    let query = Query {
        query_string: qstr,
        fields: request.fields,
        limit: request.limit,
        offset: request.offset,
        merge_strategy: request.merge_strategy.clone(),
        text_weight: request.text_weight,
        vector_weight: request.vector_weight,
        highlight: request.highlight,
    };

    let rerank_override = request.rerank.as_ref().map(|r| RerankOptions {
        enabled: r.enabled,
        candidates: r.candidates.unwrap_or(100),
        text_fields: r.text_fields.clone().unwrap_or_default(),
    });

    let result = manager.search(&collection, query, rerank_override.as_ref()).await;

    let duration = start.elapsed().as_secs_f64();

    match result {
        Ok(results) => {
            metrics::histogram!("prism_search_duration_seconds",
                "collection" => collection.clone(),
                "search_type" => "text",
            )
            .record(duration);
            metrics::counter!("prism_search_total",
                "collection" => collection,
                "search_type" => "text",
                "status" => "ok",
            )
            .increment(1);
            Ok(Json(results))
        }
        Err(e) => {
            metrics::counter!("prism_search_total",
                "collection" => collection,
                "search_type" => "text",
                "status" => "error",
            )
            .increment(1);
            tracing::error!("Search error: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn simple_search(
    State(manager): State<Arc<CollectionManager>>,
    Json(request): Json<SimpleSearchRequest>,
) -> Result<Json<SimpleSearchResponse>, StatusCode> {
    let collections = manager.list_collections();

    if collections.is_empty() {
        return Ok(Json(SimpleSearchResponse {
            results: vec![],
            total: 0,
        }));
    }

    let default_collection = collections.first().unwrap();

    let query = Query {
        query_string: request.query,
        fields: vec![],
        limit: request.limit,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
    };

    let results = manager
        .search(default_collection, query, None)
        .await
        .map_err(|e| {
            tracing::error!("Simple search error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let simple_results: Vec<SimpleSearchResult> =
        results.results.into_iter().map(result_to_simple).collect();

    Ok(Json(SimpleSearchResponse {
        results: simple_results,
        total: results.total,
    }))
}

#[derive(Deserialize)]
pub struct IndexRequest {
    pub documents: Vec<Document>,
}

#[derive(Deserialize)]
pub struct IndexQuery {
    pub pipeline: Option<String>,
}

#[derive(Serialize)]
pub struct IndexResponse {
    pub indexed: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<IndexError>,
}

#[derive(Serialize)]
pub struct IndexError {
    pub doc_id: String,
    pub error: String,
}

#[tracing::instrument(
    name = "index_documents",
    skip(state, request, query),
    fields(collection = %collection)
)]
pub async fn index_documents(
    Path(collection): Path<String>,
    axum::extract::Query(query): axum::extract::Query<IndexQuery>,
    State(state): State<AppState>,
    Json(request): Json<IndexRequest>,
) -> Result<(StatusCode, Json<IndexResponse>), StatusCode> {
    let start = std::time::Instant::now();
    let mut documents = request.documents;
    let total = documents.len();
    tracing::info!(
        "Indexing {} documents to collection '{}'",
        total,
        collection
    );

    // Apply pipeline if specified
    let mut errors = Vec::new();
    if let Some(ref pipeline_name) = query.pipeline {
        let pipeline = state.pipeline_registry.get(pipeline_name).ok_or_else(|| {
            tracing::warn!("Unknown pipeline: {}", pipeline_name);
            StatusCode::BAD_REQUEST
        })?;

        let mut processed = Vec::with_capacity(documents.len());
        for mut doc in documents {
            match pipeline.process(&mut doc) {
                Ok(()) => processed.push(doc),
                Err(e) => {
                    errors.push(IndexError {
                        doc_id: doc.id.clone(),
                        error: e.to_string(),
                    });
                }
            }
        }
        documents = processed;
    }

    let indexed = documents.len();
    let failed = errors.len();

    if !documents.is_empty() {
        state
            .manager
            .index(&collection, documents)
            .await
            .map_err(|e| {
                tracing::error!("Failed to index documents to '{}': {:?}", collection, e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    let duration = start.elapsed().as_secs_f64();
    let pipeline_label = query.pipeline.as_deref().unwrap_or("none").to_string();

    metrics::histogram!("prism_index_duration_seconds",
        "collection" => collection.clone(),
        "pipeline" => pipeline_label.clone(),
    )
    .record(duration);

    metrics::counter!("prism_index_documents_total",
        "collection" => collection.clone(),
        "status" => "ok",
    )
    .increment(indexed as u64);

    metrics::histogram!("prism_index_batch_size",
        "collection" => collection.clone(),
    )
    .record(total as f64);

    tracing::info!(
        "Indexed {}/{} documents to '{}' ({} failed)",
        indexed,
        total,
        collection,
        failed
    );
    Ok((
        StatusCode::CREATED,
        Json(IndexResponse {
            indexed,
            failed,
            errors,
        }),
    ))
}

// ============================================================================
// Pipeline Admin API (Issue #44)
// ============================================================================

#[derive(Serialize)]
pub struct PipelineInfo {
    pub name: String,
    pub description: String,
    pub processor_count: usize,
}

#[derive(Serialize)]
pub struct PipelineListResponse {
    pub pipelines: Vec<PipelineInfo>,
}

pub async fn list_pipelines(State(state): State<AppState>) -> Json<PipelineListResponse> {
    let pipelines = state
        .pipeline_registry
        .list()
        .into_iter()
        .map(|(name, desc, count)| PipelineInfo {
            name,
            description: desc,
            processor_count: count,
        })
        .collect();
    Json(PipelineListResponse { pipelines })
}

pub async fn get_document(
    Path((collection, id)): Path<(String, String)>,
    State(manager): State<Arc<CollectionManager>>,
) -> Result<Json<Option<Document>>, StatusCode> {
    manager
        .get(&collection, &id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Serialize)]
pub struct CollectionsList {
    pub collections: Vec<String>,
}

pub async fn list_collections(
    State(manager): State<Arc<CollectionManager>>,
) -> Json<CollectionsList> {
    let collections = manager.list_collections();
    Json(CollectionsList { collections })
}

pub async fn lint_schemas(
    State(manager): State<Arc<CollectionManager>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let issues = manager.lint_schemas();
    let json = serde_json::to_value(&issues).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json))
}

pub async fn health() -> StatusCode {
    StatusCode::OK
}

/// Root endpoint response
#[derive(Serialize)]
pub struct RootResponse {
    pub name: &'static str,
    pub version: &'static str,
    pub status: &'static str,
    pub tagline: &'static str,
}

/// GET /
pub async fn root() -> Json<RootResponse> {
    Json(RootResponse {
        name: "prism",
        version: env!("CARGO_PKG_VERSION"),
        status: "ok",
        tagline: "You Know, for Search",
    })
}

// ============================================================================
// Collection Metadata API (Issue #21)
// ============================================================================

/// Schema field information for API response
#[derive(Serialize)]
pub struct SchemaFieldInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub indexed: bool,
    pub stored: bool,
    pub vector_source: bool,
}

/// Collection schema response
#[derive(Serialize)]
pub struct CollectionSchemaResponse {
    pub collection: String,
    pub description: Option<String>,
    pub fields: Vec<SchemaFieldInfo>,
    pub vector_dimensions: Option<usize>,
    pub vector_field: Option<String>,
}

/// GET /collections/:collection/schema
pub async fn get_collection_schema(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
) -> Result<Json<CollectionSchemaResponse>, StatusCode> {
    let schema = manager
        .get_schema(&collection)
        .ok_or(StatusCode::NOT_FOUND)?;

    let mut fields = Vec::new();

    // Extract text fields
    if let Some(text_config) = &schema.backends.text {
        for field in &text_config.fields {
            let is_vector_source = schema
                .embedding_generation
                .as_ref()
                .map(|eg| eg.source_field == field.name)
                .unwrap_or(false);

            fields.push(SchemaFieldInfo {
                name: field.name.clone(),
                field_type: format!("{:?}", field.field_type).to_lowercase(),
                indexed: field.indexed,
                stored: field.stored,
                vector_source: is_vector_source,
            });
        }
    }

    let (vector_dimensions, vector_field) = if let Some(vector_config) = &schema.backends.vector {
        (
            Some(vector_config.dimension),
            Some(vector_config.embedding_field.clone()),
        )
    } else {
        (None, None)
    };

    Ok(Json(CollectionSchemaResponse {
        collection: schema.collection.clone(),
        description: schema.description.clone(),
        fields,
        vector_dimensions,
        vector_field,
    }))
}

/// Collection stats response
#[derive(Serialize)]
pub struct CollectionStatsResponse {
    pub collection: String,
    pub document_count: usize,
    pub storage_bytes: usize,
}

/// GET /collections/:collection/stats
pub async fn get_collection_stats(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
) -> Result<Json<CollectionStatsResponse>, StatusCode> {
    // Check if collection exists
    if manager.get_schema(&collection).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let stats = manager.stats(&collection).await.map_err(|e| {
        tracing::error!(
            "Failed to get stats for collection '{}': {:?}",
            collection,
            e
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(CollectionStatsResponse {
        collection,
        document_count: stats.document_count,
        storage_bytes: stats.size_bytes,
    }))
}

// ============================================================================
// Cache Stats API (Issue #22)
// ============================================================================

/// Cache stats response
#[derive(Serialize)]
pub struct CacheStatsResponse {
    pub total_entries: usize,
    pub total_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
}

/// GET /stats/cache
pub async fn get_cache_stats(
    State(manager): State<Arc<CollectionManager>>,
) -> Result<Json<CacheStatsResponse>, StatusCode> {
    let stats = manager
        .cache_stats()
        .await
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    Ok(Json(CacheStatsResponse {
        total_entries: stats.total_entries,
        total_bytes: stats.total_bytes,
        hits: stats.hits,
        misses: stats.misses,
        hit_rate: stats.hit_rate(),
    }))
}

/// Server info response
#[derive(Serialize)]
pub struct ServerInfoResponse {
    pub version: String,
    pub prism_version: String,
}

/// GET /stats/server
pub async fn get_server_info() -> Json<ServerInfoResponse> {
    Json(ServerInfoResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        prism_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// ============================================================================
// Aggregations API (Issue #23)
// ============================================================================

use crate::aggregations::{AggregationRequest as AggRequest, AggregationResult as AggResult};
use crate::backends::text::{ReconstructedDocument, SegmentsInfo, TermInfo};

/// Aggregation API request
#[derive(Deserialize)]
pub struct AggregateRequest {
    /// Optional filter query (if empty, aggregates all documents)
    #[serde(default)]
    pub query: Option<String>,
    /// List of aggregations to run
    pub aggregations: Vec<AggRequest>,
    /// Max documents to scan (default 10000)
    #[serde(default = "default_scan_limit")]
    pub scan_limit: usize,
}

fn default_scan_limit() -> usize {
    10000
}

/// Aggregation API response
#[derive(Serialize)]
pub struct AggregateResponse {
    pub results: HashMap<String, AggResult>,
    pub took_ms: u64,
}

/// POST /collections/:collection/aggregate
pub async fn aggregate(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
    Json(request): Json<AggregateRequest>,
) -> Result<Json<AggregateResponse>, StatusCode> {
    let start = std::time::Instant::now();

    // Check if collection exists
    if manager.get_schema(&collection).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Build search query
    let query_string = request.query.unwrap_or_else(|| "*".to_string());
    let query = Query {
        query_string,
        fields: vec![],
        limit: request.scan_limit,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
    };

    // Use search_with_aggs to run aggregations in the text backend
    let agg_results = manager
        .search_with_aggs(&collection, &query, request.aggregations)
        .await
        .map_err(|e| {
            tracing::error!("Aggregation error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let took_ms = start.elapsed().as_millis() as u64;

    Ok(Json(AggregateResponse {
        results: agg_results.aggregations,
        took_ms,
    }))
}

// ============================================================================
// Index Inspection API (Issue #24)
// ============================================================================

/// Top terms response
#[derive(Serialize)]
pub struct TopTermsResponse {
    pub field: String,
    pub terms: Vec<TermInfo>,
}

/// Query params for terms endpoint
#[derive(Deserialize)]
pub struct TermsQuery {
    #[serde(default = "default_terms_limit")]
    pub limit: usize,
}

fn default_terms_limit() -> usize {
    25
}

/// GET /collections/:collection/terms/:field
pub async fn get_top_terms(
    Path((collection, field)): Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<TermsQuery>,
    State(manager): State<Arc<CollectionManager>>,
) -> Result<Json<TopTermsResponse>, StatusCode> {
    // Check if collection exists
    if manager.get_schema(&collection).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let terms = manager
        .get_top_terms(&collection, &field, params.limit)
        .map_err(|e| {
            tracing::error!(
                "Failed to get top terms for {}/{}: {:?}",
                collection,
                field,
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(TopTermsResponse { field, terms }))
}

/// GET /collections/:collection/segments
pub async fn get_segments(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
) -> Result<Json<SegmentsInfo>, StatusCode> {
    // Check if collection exists
    if manager.get_schema(&collection).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let segments = manager.get_segments(&collection).map_err(|e| {
        tracing::error!("Failed to get segments for {}: {:?}", collection, e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(segments))
}

/// GET /collections/:collection/doc/:id/reconstruct
pub async fn reconstruct_document(
    Path((collection, id)): Path<(String, String)>,
    State(manager): State<Arc<CollectionManager>>,
) -> Result<Json<Option<ReconstructedDocument>>, StatusCode> {
    // Check if collection exists
    if manager.get_schema(&collection).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let doc = manager
        .reconstruct_document(&collection, &id)
        .map_err(|e| {
            tracing::error!(
                "Failed to reconstruct document {}/{}: {:?}",
                collection,
                id,
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(doc))
}

// ============================================================================
// Suggestions / Autocomplete API (Issue #47)
// ============================================================================

fn default_suggest_size() -> usize {
    5
}

fn default_max_distance() -> usize {
    2
}

/// Request body for POST /collections/:collection/_suggest
#[derive(Deserialize)]
pub struct SuggestRequest {
    pub prefix: String,
    pub field: String,
    #[serde(default = "default_suggest_size")]
    pub size: usize,
    #[serde(default)]
    pub fuzzy: bool,
    #[serde(default = "default_max_distance")]
    pub max_distance: usize,
}

/// A single suggestion entry in the response
#[derive(Serialize)]
pub struct SuggestionEntry {
    pub term: String,
    pub score: f32,
    pub doc_freq: u64,
}

/// Response body for POST /collections/:collection/_suggest
#[derive(Serialize)]
pub struct SuggestResponse {
    pub suggestions: Vec<SuggestionEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub did_you_mean: Option<String>,
}

/// POST /collections/:collection/_suggest
pub async fn suggest(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
    Json(req): Json<SuggestRequest>,
) -> Result<Json<SuggestResponse>, StatusCode> {
    if manager.get_schema(&collection).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let entries = manager
        .suggest(
            &collection,
            &req.field,
            &req.prefix,
            req.size,
            req.fuzzy,
            req.max_distance,
        )
        .map_err(|e| {
            tracing::error!(
                "Failed to suggest for {}/{}: {:?}",
                collection,
                req.field,
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let suggestions: Vec<SuggestionEntry> = entries
        .into_iter()
        .map(|e| SuggestionEntry {
            term: e.term,
            score: e.score,
            doc_freq: e.doc_freq,
        })
        .collect();

    // Populate did_you_mean when fuzzy is enabled
    let did_you_mean = if req.fuzzy {
        // Gather vocabulary from the top terms for the field
        let top_terms = manager
            .get_top_terms(&collection, &req.field, 1000)
            .unwrap_or_default();
        let vocabulary: Vec<String> = top_terms.into_iter().map(|t| t.term).collect();
        let corrections = crate::query::suggestions::suggest_query_corrections(
            &req.prefix,
            &vocabulary,
            req.max_distance,
        );
        corrections.into_iter().next()
    } else {
        None
    };

    Ok(Json(SuggestResponse {
        suggestions,
        did_you_mean,
    }))
}

// ============================================================================
// More Like This API (Issue #48)
// ============================================================================

fn default_min_term_freq() -> usize {
    2
}

fn default_min_doc_freq() -> u64 {
    5
}

fn default_max_query_terms() -> usize {
    25
}

fn default_mlt_size() -> usize {
    10
}

/// Like target — either a document ID or raw text
#[derive(Deserialize)]
pub struct MltLike {
    #[serde(rename = "_id")]
    pub id: Option<String>,
}

/// Request body for POST /collections/:collection/_mlt
#[derive(Deserialize)]
pub struct MltRequest {
    /// Find docs like this document
    #[serde(default)]
    pub like: Option<MltLike>,
    /// Or find docs like this text
    #[serde(default)]
    pub like_text: Option<String>,
    /// Fields to extract terms from
    #[serde(default)]
    pub fields: Vec<String>,
    /// Minimum term frequency in source doc
    #[serde(default = "default_min_term_freq")]
    pub min_term_freq: usize,
    /// Minimum document frequency in the index
    #[serde(default = "default_min_doc_freq")]
    pub min_doc_freq: u64,
    /// Maximum number of query terms to use
    #[serde(default = "default_max_query_terms")]
    pub max_query_terms: usize,
    /// Number of results to return
    #[serde(default = "default_mlt_size")]
    pub size: usize,
}

/// POST /collections/:collection/_mlt
pub async fn more_like_this(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
    Json(req): Json<MltRequest>,
) -> Result<Json<SearchResults>, StatusCode> {
    if manager.get_schema(&collection).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let doc_id = req.like.as_ref().and_then(|l| l.id.as_deref());
    let like_text = req.like_text.as_deref();

    if doc_id.is_none() && like_text.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let results = manager
        .more_like_this(
            &collection,
            doc_id,
            like_text,
            &req.fields,
            req.min_term_freq,
            req.min_doc_freq,
            req.max_query_terms,
            req.size,
        )
        .map_err(|e| {
            tracing::error!("MLT error for {}: {:?}", collection, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(results))
}

// ============================================================================
// Multi-Collection Search API (Issue #74)
// ============================================================================

use crate::collection::MultiSearchResults;

/// Request body for POST /_msearch
#[derive(Deserialize)]
pub struct MultiSearchRequest {
    /// Collections to search (supports wildcards like "logs-*")
    pub collections: Vec<String>,
    /// Free-form text query
    #[serde(default)]
    pub query: Option<String>,
    /// Optional explicit vector query
    #[serde(default)]
    pub vector: Option<Vec<f32>>,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    /// RRF constant for result merging (default: 60)
    #[serde(default)]
    pub rrf_k: Option<usize>,
    /// Optional highlight configuration
    #[serde(default)]
    pub highlight: Option<HighlightConfig>,
}

/// POST /_msearch - Multi-collection search
#[tracing::instrument(
    name = "msearch",
    skip(manager, request),
    fields(collections = ?request.collections)
)]
pub async fn multi_search(
    State(manager): State<Arc<CollectionManager>>,
    Json(request): Json<MultiSearchRequest>,
) -> Result<Json<MultiSearchResults>, StatusCode> {
    let start = std::time::Instant::now();

    let qstr = if let Some(vec) = request.vector.clone() {
        serde_json::to_string(&vec).unwrap_or_default()
    } else {
        request.query.clone().unwrap_or_default()
    };

    let query = Query {
        query_string: qstr,
        fields: request.fields,
        limit: request.limit,
        offset: request.offset,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: request.highlight,
    };

    let result = manager
        .multi_search(&request.collections, query, request.rrf_k)
        .await;

    let duration = start.elapsed().as_secs_f64();

    match result {
        Ok(results) => {
            metrics::histogram!("prism_msearch_duration_seconds").record(duration);
            metrics::counter!("prism_msearch_total",
                "status" => "ok",
            )
            .increment(1);
            Ok(Json(results))
        }
        Err(e) => {
            metrics::counter!("prism_msearch_total",
                "status" => "error",
            )
            .increment(1);
            tracing::error!("Multi-search error: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// POST /:collections/_search - Search with comma-separated collections in path
/// Supports: /products,articles/_search or /logs-2026-*/_search
#[tracing::instrument(
    name = "multi_index_search",
    skip(manager, request),
    fields(collections = %collections)
)]
pub async fn multi_index_search(
    Path(collections): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<MultiSearchResults>, StatusCode> {
    let start = std::time::Instant::now();

    // Parse comma-separated collection names
    let collection_list: Vec<String> = collections
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if collection_list.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let qstr = if let Some(vec) = request.vector.clone() {
        serde_json::to_string(&vec).unwrap_or_default()
    } else {
        request.query.clone().unwrap_or_default()
    };

    let query = Query {
        query_string: qstr,
        fields: request.fields,
        limit: request.limit,
        offset: request.offset,
        merge_strategy: request.merge_strategy,
        text_weight: request.text_weight,
        vector_weight: request.vector_weight,
        highlight: request.highlight,
    };

    let result = manager.multi_search(&collection_list, query, None).await;

    let duration = start.elapsed().as_secs_f64();

    match result {
        Ok(results) => {
            metrics::histogram!("prism_msearch_duration_seconds").record(duration);
            metrics::counter!("prism_msearch_total",
                "status" => "ok",
            )
            .increment(1);
            Ok(Json(results))
        }
        Err(e) => {
            metrics::counter!("prism_msearch_total",
                "status" => "error",
            )
            .increment(1);
            tracing::error!("Multi-index search error: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ============================================================================
// ILM API (Issue #45)
// ============================================================================

use crate::ilm::{
    IlmExplain, IlmIndexStatus, IlmManager, IlmPolicy, Phase, RolloverResult, TransitionResult,
};

/// ILM-enabled AppState with IlmManager
#[derive(Clone)]
pub struct IlmAppState {
    pub manager: Arc<CollectionManager>,
    pub ilm_manager: Option<Arc<IlmManager>>,
}

/// GET /_ilm/policy - List all ILM policies
pub async fn list_ilm_policies(
    State(state): State<IlmAppState>,
) -> Result<Json<Vec<IlmPolicy>>, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let policies = ilm.list_policies().await;
    Ok(Json(policies))
}

/// GET /_ilm/policy/:name - Get a specific policy
pub async fn get_ilm_policy(
    Path(name): Path<String>,
    State(state): State<IlmAppState>,
) -> Result<Json<IlmPolicy>, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let policy = ilm.get_policy(&name).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(policy))
}

/// PUT /_ilm/policy/:name - Create or update a policy
#[derive(Deserialize)]
pub struct CreatePolicyRequest {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub rollover_max_size: Option<String>,
    #[serde(default)]
    pub rollover_max_age: Option<String>,
    #[serde(default)]
    pub rollover_max_docs: Option<usize>,
    #[serde(default)]
    pub phases: crate::ilm::config::IlmPhasesConfig,
}

pub async fn create_ilm_policy(
    Path(name): Path<String>,
    State(state): State<IlmAppState>,
    Json(request): Json<CreatePolicyRequest>,
) -> Result<StatusCode, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    let config = crate::ilm::IlmPolicyConfig {
        description: request.description.unwrap_or_default(),
        rollover_max_size: request.rollover_max_size,
        rollover_max_age: request.rollover_max_age,
        rollover_max_docs: request.rollover_max_docs,
        phases: request.phases,
    };

    let policy = config.to_policy(name);
    ilm.upsert_policy(policy).await.map_err(|e| {
        tracing::error!("Failed to create ILM policy: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::CREATED)
}

/// DELETE /_ilm/policy/:name - Delete a policy
pub async fn delete_ilm_policy(
    Path(name): Path<String>,
    State(state): State<IlmAppState>,
) -> Result<StatusCode, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    ilm.delete_policy(&name)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete ILM policy: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(StatusCode::NO_CONTENT)
}

/// GET /_ilm/status - Get status of all managed indexes
pub async fn get_ilm_status(
    State(state): State<IlmAppState>,
) -> Result<Json<Vec<IlmIndexStatus>>, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let status = ilm.get_status().await;
    Ok(Json(status))
}

/// GET /:index/_ilm/explain - Explain ILM state for an index
pub async fn ilm_explain(
    Path(collection): Path<String>,
    State(state): State<IlmAppState>,
) -> Result<Json<IlmExplain>, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    ilm.explain(&collection).await.map(Json).map_err(|e| {
        tracing::error!("ILM explain error: {:?}", e);
        StatusCode::NOT_FOUND
    })
}

/// POST /:index/_rollover - Trigger manual rollover
pub async fn ilm_rollover(
    Path(index): Path<String>,
    State(state): State<IlmAppState>,
) -> Result<Json<RolloverResult>, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    ilm.rollover(&index).await.map(Json).map_err(|e| {
        tracing::error!("ILM rollover error: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// POST /:index/_ilm/move/:phase - Force phase transition
pub async fn ilm_move_phase(
    Path((collection, phase_str)): Path<(String, String)>,
    State(state): State<IlmAppState>,
) -> Result<Json<TransitionResult>, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    let phase: Phase = phase_str.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    ilm.move_to_phase(&collection, phase)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("ILM move phase error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

/// POST /:index/_ilm/attach - Attach an ILM policy to a collection
#[derive(Deserialize)]
pub struct AttachPolicyRequest {
    pub policy: String,
}

pub async fn ilm_attach_policy(
    Path(collection): Path<String>,
    State(state): State<IlmAppState>,
    Json(request): Json<AttachPolicyRequest>,
) -> Result<StatusCode, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    // Extract index name from collection name (e.g., "logs-2026.01.29-000001" -> "logs")
    let index_name = collection.split('-').next().unwrap_or(&collection);

    ilm.attach_policy(index_name, &collection, &request.policy)
        .await
        .map_err(|e| {
            tracing::error!("ILM attach policy error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::OK)
}

/// GET /_aliases - List all aliases
pub async fn list_aliases(
    State(state): State<IlmAppState>,
) -> Result<Json<Vec<crate::ilm::IndexAlias>>, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let aliases = ilm.alias_manager().list().await;
    Ok(Json(aliases))
}

/// Alias update action
#[derive(Deserialize)]
pub struct AliasAction {
    #[serde(default)]
    pub add: Option<AliasActionParams>,
    #[serde(default)]
    pub remove: Option<AliasActionParams>,
}

#[derive(Deserialize)]
pub struct AliasActionParams {
    pub alias: String,
    pub index: String,
}

/// PUT /_aliases - Bulk alias updates
#[derive(Deserialize)]
pub struct BulkAliasRequest {
    pub actions: Vec<AliasAction>,
}

pub async fn update_aliases(
    State(state): State<IlmAppState>,
    Json(request): Json<BulkAliasRequest>,
) -> Result<StatusCode, StatusCode> {
    let ilm = state.ilm_manager.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    let mut adds = Vec::new();
    let mut removes = Vec::new();

    for action in request.actions {
        if let Some(add) = action.add {
            adds.push((add.alias, add.index));
        }
        if let Some(remove) = action.remove {
            removes.push((remove.alias, remove.index));
        }
    }

    ilm.alias_manager()
        .atomic_update(adds, removes)
        .await
        .map_err(|e| {
            tracing::error!("Alias update error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::OK)
}

// ============================================================================
// Index Template endpoints
// ============================================================================

use crate::templates::{IndexTemplate, TemplateManager};

/// App state for template routes
#[derive(Clone)]
pub struct TemplateAppState {
    pub manager: Arc<CollectionManager>,
    pub template_manager: Option<Arc<TemplateManager>>,
}

/// GET /_template - List all templates
pub async fn list_templates(
    State(state): State<TemplateAppState>,
) -> Result<Json<ListTemplatesResponse>, StatusCode> {
    let tm = state
        .template_manager
        .as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;
    let templates = tm.list_templates().await;
    Ok(Json(ListTemplatesResponse { templates }))
}

#[derive(Serialize)]
pub struct ListTemplatesResponse {
    pub templates: Vec<IndexTemplate>,
}

/// GET /_template/:name - Get a specific template
pub async fn get_template(
    Path(name): Path<String>,
    State(state): State<TemplateAppState>,
) -> Result<Json<IndexTemplate>, StatusCode> {
    let tm = state
        .template_manager
        .as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;
    let template = tm.get_template(&name).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(template))
}

/// PUT /_template/:name - Create or update a template
#[derive(Deserialize)]
pub struct CreateTemplateRequest {
    pub index_patterns: Vec<String>,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub settings: crate::templates::TemplateSettings,
    #[serde(default)]
    pub schema: crate::templates::TemplateSchema,
    #[serde(default)]
    pub aliases: HashMap<String, crate::templates::AliasDefinition>,
}

pub async fn put_template(
    Path(name): Path<String>,
    State(state): State<TemplateAppState>,
    Json(request): Json<CreateTemplateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let tm = state.template_manager.as_ref().ok_or((
        StatusCode::NOT_FOUND,
        "Template manager not enabled".to_string(),
    ))?;

    let template = IndexTemplate {
        name,
        index_patterns: request.index_patterns,
        priority: request.priority,
        settings: request.settings,
        schema: request.schema,
        aliases: request.aliases,
    };

    tm.put_template(template).await.map_err(|e| {
        tracing::error!("Failed to create template: {:?}", e);
        (StatusCode::BAD_REQUEST, e.to_string())
    })?;

    Ok(StatusCode::CREATED)
}

/// DELETE /_template/:name - Delete a template
pub async fn delete_template(
    Path(name): Path<String>,
    State(state): State<TemplateAppState>,
) -> Result<StatusCode, StatusCode> {
    let tm = state
        .template_manager
        .as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;

    match tm.delete_template(&name).await {
        Ok(Some(_)) => Ok(StatusCode::OK),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("Failed to delete template: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// GET /_template/_simulate/:index - Simulate template matching
#[derive(Serialize)]
pub struct SimulateTemplateResponse {
    pub matched: bool,
    pub template: Option<IndexTemplate>,
    pub matched_pattern: Option<String>,
}

pub async fn simulate_template(
    Path(index_name): Path<String>,
    State(state): State<TemplateAppState>,
) -> Result<Json<SimulateTemplateResponse>, StatusCode> {
    let tm = state
        .template_manager
        .as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;

    match tm.find_best_match(&index_name).await {
        Some(m) => Ok(Json(SimulateTemplateResponse {
            matched: true,
            template: Some(m.template),
            matched_pattern: Some(m.matched_pattern),
        })),
        None => Ok(Json(SimulateTemplateResponse {
            matched: false,
            template: None,
            matched_pattern: None,
        })),
    }
}

// ============================================================================
// Encrypted Export API (Issue #75) - Emergency data offloading
// ============================================================================

use crate::export::{export_encrypted, import_encrypted, EncryptedExportConfig};

/// App state for export routes
#[derive(Clone)]
pub struct ExportAppState {
    pub manager: Arc<CollectionManager>,
    pub data_dir: std::path::PathBuf,
}

/// Request for encrypted export
#[derive(Deserialize)]
pub struct EncryptedExportRequest {
    /// Collection to export
    pub collection: String,
    /// Encryption key (hex-encoded, 64 characters for AES-256)
    pub key: String,
    /// Output file path
    pub output_path: String,
}

/// Response for encrypted export
#[derive(Serialize)]
pub struct EncryptedExportResponse {
    pub success: bool,
    pub collection: String,
    pub output_path: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// POST /_admin/export/encrypted - Export collection with encryption
///
/// Use this for emergency data offloading to untrusted cloud storage.
/// The key is only held in memory during the export operation.
///
/// # Security
/// - Requires admin privileges
/// - Key is never logged or persisted
/// - Use HTTPS in production
pub async fn encrypted_export(
    State(state): State<ExportAppState>,
    Json(request): Json<EncryptedExportRequest>,
) -> Result<Json<EncryptedExportResponse>, (StatusCode, Json<EncryptedExportResponse>)> {
    // Parse encryption config from hex key
    let config = match EncryptedExportConfig::from_hex(&request.key) {
        Ok(c) => c,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(EncryptedExportResponse {
                    success: false,
                    collection: request.collection,
                    output_path: request.output_path,
                    size_bytes: 0,
                    error: Some(format!("Invalid key: {}", e)),
                }),
            ));
        }
    };

    let output_path = std::path::PathBuf::from(&request.output_path);

    // Perform encrypted export
    match export_encrypted(
        &state.data_dir,
        &request.collection,
        &output_path,
        config,
        None,
    ) {
        Ok(metadata) => Ok(Json(EncryptedExportResponse {
            success: true,
            collection: request.collection,
            output_path: request.output_path,
            size_bytes: metadata.size_bytes,
            error: None,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(EncryptedExportResponse {
                success: false,
                collection: request.collection,
                output_path: request.output_path,
                size_bytes: 0,
                error: Some(e.to_string()),
            }),
        )),
    }
}

/// Request for encrypted import
#[derive(Deserialize)]
pub struct EncryptedImportRequest {
    /// Input encrypted file path
    pub input_path: String,
    /// Decryption key (hex-encoded, 64 characters for AES-256)
    pub key: String,
    /// Optional: rename collection during import
    #[serde(default)]
    pub target_collection: Option<String>,
}

/// Response for encrypted import
#[derive(Serialize)]
pub struct EncryptedImportResponse {
    pub success: bool,
    pub collection: String,
    pub files_extracted: u64,
    pub bytes_extracted: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// POST /_admin/import/encrypted - Import collection from encrypted backup
///
/// Restores a collection from an encrypted export.
///
/// # Security
/// - Requires admin privileges
/// - Key is never logged or persisted
pub async fn encrypted_import(
    State(state): State<ExportAppState>,
    Json(request): Json<EncryptedImportRequest>,
) -> Result<Json<EncryptedImportResponse>, (StatusCode, Json<EncryptedImportResponse>)> {
    // Parse encryption config from hex key
    let config = match EncryptedExportConfig::from_hex(&request.key) {
        Ok(c) => c,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(EncryptedImportResponse {
                    success: false,
                    collection: String::new(),
                    files_extracted: 0,
                    bytes_extracted: 0,
                    error: Some(format!("Invalid key: {}", e)),
                }),
            ));
        }
    };

    let input_path = std::path::PathBuf::from(&request.input_path);

    // Perform encrypted import
    match import_encrypted(
        &state.data_dir,
        &input_path,
        config,
        request.target_collection.as_deref(),
        None,
    ) {
        Ok(result) => Ok(Json(EncryptedImportResponse {
            success: true,
            collection: result.collection,
            files_extracted: result.files_extracted,
            bytes_extracted: result.bytes_extracted,
            error: None,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(EncryptedImportResponse {
                success: false,
                collection: String::new(),
                files_extracted: 0,
                bytes_extracted: 0,
                error: Some(e.to_string()),
            }),
        )),
    }
}

/// POST /_admin/encryption/generate-key - Generate a new encryption key
///
/// Generates a cryptographically secure AES-256 key.
/// Save this key securely (e.g., in a secrets manager).
#[derive(Serialize)]
pub struct GenerateKeyResponse {
    /// Hex-encoded 256-bit key (64 characters)
    pub key: String,
    /// Key length in bytes
    pub key_bytes: usize,
    /// Algorithm info
    pub algorithm: String,
}

pub async fn generate_encryption_key() -> Json<GenerateKeyResponse> {
    let config = EncryptedExportConfig::generate();
    Json(GenerateKeyResponse {
        key: config.to_hex(),
        key_bytes: 32,
        algorithm: "AES-256-GCM".to_string(),
    })
}

// ============================================================================
// Detach/Attach API (Issue #57) - Live collection detach and re-attach
// ============================================================================

use crate::collection::detach::{
    attach_collection, detach_collection, AttachResult, AttachSource, DetachDestination,
    DetachResult,
};

#[derive(Deserialize)]
pub struct DetachRequest {
    pub destination: DetachDestination,
    #[serde(default)]
    pub delete_data: bool,
}

/// POST /_admin/collections/:name/detach
pub async fn collection_detach(
    State(state): State<ExportAppState>,
    Path(name): Path<String>,
    Json(request): Json<DetachRequest>,
) -> Result<Json<DetachResult>, (StatusCode, String)> {
    match detach_collection(
        &state.manager,
        &state.data_dir,
        &name,
        &request.destination,
        request.delete_data,
        None,
    )
    .await
    {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

#[derive(Deserialize)]
pub struct AttachRequest {
    pub source: AttachSource,
    #[serde(default)]
    pub target_collection: Option<String>,
}

/// POST /_admin/collections/attach
pub async fn collection_attach(
    State(state): State<ExportAppState>,
    Json(request): Json<AttachRequest>,
) -> Result<Json<AttachResult>, (StatusCode, String)> {
    match attach_collection(
        &state.manager,
        &state.data_dir,
        &request.source,
        request.target_collection.as_deref(),
        None,
    )
    .await
    {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
