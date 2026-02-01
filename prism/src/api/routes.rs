use crate::backends::{Document, Query, SearchResults, SearchResult};
use crate::collection::CollectionManager;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
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
    let title = result.fields.get("title").and_then(|v| v.as_str()).map(String::from);
    let url = result.fields.get("url").or_else(|| result.fields.get("link"))
        .and_then(|v| v.as_str()).map(String::from);
    let snippet = result.fields.get("snippet").or_else(|| result.fields.get("content"))
        .or_else(|| result.fields.get("description"))
        .and_then(|v| v.as_str()).map(String::from);

    SimpleSearchResult {
        id: result.id,
        title,
        url,
        snippet,
        score: result.score,
    }
}

pub async fn search(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResults>, StatusCode> {
    let qstr = if let Some(vec) = request.vector.clone() {
        serde_json::to_string(&vec).unwrap_or_default()
    } else { request.query.clone().unwrap_or_default() };

    let query = Query {
        query_string: qstr,
        fields: request.fields,
        limit: request.limit,
        offset: request.offset,
        merge_strategy: request.merge_strategy.clone(),
        text_weight: request.text_weight,
        vector_weight: request.vector_weight,
    };

    manager
        .search(&collection, query)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("Search error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })
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
    };

    let results = manager
        .search(default_collection, query)
        .await
        .map_err(|e| {
            tracing::error!("Simple search error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let simple_results: Vec<SimpleSearchResult> = results.results.into_iter().map(result_to_simple).collect();

    Ok(Json(SimpleSearchResponse {
        results: simple_results,
        total: results.total,
    }))
}

#[derive(Deserialize)]
pub struct IndexRequest {
    pub documents: Vec<Document>,
}

pub async fn index_documents(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
    Json(request): Json<IndexRequest>,
) -> Result<StatusCode, StatusCode> {
    let doc_count = request.documents.len();
    tracing::info!("Indexing {} documents to collection '{}'", doc_count, collection);

    manager
        .index(&collection, request.documents)
        .await
        .map(|_| {
            tracing::info!("Successfully indexed {} documents to '{}'", doc_count, collection);
            StatusCode::CREATED
        })
        .map_err(|e| {
            tracing::error!("Failed to index {} documents to '{}': {:?}", doc_count, collection, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })
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
        (Some(vector_config.dimension), Some(vector_config.embedding_field.clone()))
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

    let stats = manager
        .stats(&collection)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get stats for collection '{}': {:?}", collection, e);
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

use crate::query::aggregations::{AggregationRequest, AggregationType, AggregationResult};
use crate::backends::text::{TermInfo, SegmentsInfo, ReconstructedDocument};

/// Aggregation API request
#[derive(Deserialize)]
pub struct AggregateRequest {
    /// Optional filter query (if empty, aggregates all documents)
    #[serde(default)]
    pub query: Option<String>,
    /// List of aggregations to run
    pub aggregations: Vec<AggregationRequest>,
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
    pub results: Vec<AggregationResult>,
    pub scanned_docs: usize,
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

    // Build search query to get documents
    let query_string = request.query.unwrap_or_else(|| "*".to_string());
    let query = Query {
        query_string,
        fields: vec![],
        limit: request.scan_limit,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
    };

    // Execute search to get documents
    let search_results = manager
        .search(&collection, query)
        .await
        .map_err(|e| {
            tracing::error!("Aggregation search error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let scanned_docs = search_results.results.len();

    // Run each aggregation
    let mut results = Vec::new();
    for agg_req in request.aggregations {
        let field_values: Vec<String> = search_results
            .results
            .iter()
            .filter_map(|hit| {
                hit.fields
                    .get(&agg_req.field)
                    .and_then(|v: &serde_json::Value| {
                        if v.is_string() {
                            Some(v.as_str().unwrap().to_string())
                        } else {
                            Some(v.to_string())
                        }
                    })
            })
            .collect();

        let mut result = match agg_req.agg_type {
            AggregationType::Terms => {
                crate::query::aggregations::terms::aggregate_terms(field_values, agg_req.size)
            }
            // DateHistogram, Range, Stats require more complex type handling
            // Not yet implemented for REST API - return empty result
            AggregationType::DateHistogram | AggregationType::Range | AggregationType::Stats => {
                AggregationResult {
                    field: agg_req.field.clone(),
                    buckets: vec![],
                }
            }
        };

        result.field = agg_req.field;
        results.push(result);
    }

    let took_ms = start.elapsed().as_millis() as u64;

    Ok(Json(AggregateResponse {
        results,
        scanned_docs,
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
            tracing::error!("Failed to get top terms for {}/{}: {:?}", collection, field, e);
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

    let segments = manager
        .get_segments(&collection)
        .map_err(|e| {
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
            tracing::error!("Failed to reconstruct document {}/{}: {:?}", collection, id, e);
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
        .suggest(&collection, &req.field, &req.prefix, req.size, req.fuzzy, req.max_distance)
        .map_err(|e| {
            tracing::error!("Failed to suggest for {}/{}: {:?}", collection, req.field, e);
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

    Ok(Json(SuggestResponse { suggestions, did_you_mean }))
}
