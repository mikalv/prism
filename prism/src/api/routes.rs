use crate::backends::{Document, Query, SearchResults};
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
