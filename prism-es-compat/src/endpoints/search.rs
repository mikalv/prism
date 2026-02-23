//! ES-compatible _search endpoint

use crate::error::EsCompatError;
use crate::query::{EsSearchRequest, QueryTranslator};
use crate::response::{EsSearchResponse, ResponseMapper};
use axum::extract::{Path, State};
use axum::Json;
use prism::backends::SearchResult;
use prism::collection::CollectionManager;
use std::sync::Arc;
use std::time::Instant;

/// State for ES compat handlers
#[derive(Clone)]
pub struct EsCompatState {
    pub manager: Arc<CollectionManager>,
}

/// POST /_elastic/_search - Search across all indices
/// POST /_elastic/{index}/_search - Search specific index
pub async fn search_handler(
    State(state): State<EsCompatState>,
    index: Option<Path<String>>,
    Json(request): Json<EsSearchRequest>,
) -> Result<Json<EsSearchResponse>, EsCompatError> {
    let start = Instant::now();

    let index_name = index.map(|p| p.0).unwrap_or_else(|| "*".to_string());

    // Expand index pattern to collections (sync method)
    let collections = state
        .manager
        .expand_collection_patterns(std::slice::from_ref(&index_name));

    if collections.is_empty() {
        return Err(EsCompatError::IndexNotFound(index_name));
    }

    // Get default fields from first collection's schema (sync method)
    let default_fields = get_text_fields(&state.manager, &collections[0]);

    // Translate ES query to Prism
    let (query, aggregations) = QueryTranslator::translate(&request, &default_fields)?;

    // Execute search
    let results = if collections.len() == 1 {
        state
            .manager
            .search_with_aggs(&collections[0], &query, aggregations)
            .await?
    } else {
        // Multi-collection search (without aggregations for now)
        let multi_results = state
            .manager
            .multi_search(&collections, query, None) // rrf_k = None
            .await?;

        // Convert MultiSearchResults to SearchResultsWithAggs
        prism::backends::SearchResultsWithAggs {
            results: multi_results
                .results
                .into_iter()
                .map(|r| SearchResult {
                    id: r.id,
                    score: r.score,
                    fields: r.fields,
                    highlight: r.highlight,
                })
                .collect(),
            total: multi_results.total as u64,
            aggregations: std::collections::HashMap::new(), // TODO: aggregate aggs
        }
    };

    let took_ms = start.elapsed().as_millis() as u64;

    // Map to ES response format
    let response = ResponseMapper::map_search_results(&index_name, results, took_ms);

    Ok(Json(response))
}

/// Get text field names from collection schema
pub(crate) fn get_text_fields(manager: &CollectionManager, collection: &str) -> Vec<String> {
    manager
        .get_schema(collection)
        .map(|schema| {
            schema
                .backends
                .text
                .as_ref()
                .map(|t| t.fields.iter().map(|f| f.name.clone()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}
