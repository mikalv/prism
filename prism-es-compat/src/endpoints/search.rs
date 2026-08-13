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
use std::path::PathBuf;

use crate::endpoints::templates::TemplateStore;

/// State for ES compat handlers
#[derive(Clone)]
pub struct EsCompatState {
    pub manager: Arc<CollectionManager>,
    pub templates: TemplateStore,
    /// In-memory ILM policy store (ES-compat shim).
    pub ilm: crate::endpoints::ilm::IlmStore,
    /// Directory for persisted ES-compat metadata (aliases.json, templates.json).
    pub data_dir: PathBuf,
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

    // If a PIT id is provided, decode the collection from it. PIT searches have
    // no index in the URL, so the collection must come from the PIT id.
    let index_name = if let Some(ref pit) = request.pit {
        decode_pit_collection(&pit.id).unwrap_or(index_name)
    } else {
        index_name
    };

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

    // Log the raw ES request body + translated query string (ground truth for
    // debugging client integrations like Kibana). Parse outcome is logged
    // separately at the text-parse layer.
    prism::query_log::log(
        &prism::query_log::QueryLogEntry::new("es-compat")
            .op("search")
            .index(&index_name)
            .collection(&collections[0])
            .query(&query.query_string)
            .raw(serde_json::to_value(&request).unwrap_or(serde_json::Value::Null)),
    );;

    // Execute search. The ES-compat layer degrades query parse/syntax errors
    // to an empty result set instead of a hard 400. Tantivy's query parser is
    // stricter than Elasticsearch, so many legitimate ES/Kibana queries (e.g.
    // Kibana saved-object migration field names that contain colons) cannot be
    // expressed in Tantivy's query-string syntax. Returning empty results lets
    // clients like Kibana proceed; for a fresh index this is also the correct
    // answer (no matching documents). The original parse error is preserved in
    // the query log via the text-parse layer.
    let search_result = if collections.len() == 1 {
        state
            .manager
            .search_with_aggs(&collections[0], &query, aggregations)
            .await
    } else {
        // Multi-collection search (without aggregations for now)
        state
            .manager
            .multi_search(&collections, query, None) // rrf_k = None
            .await
            .map(|multi_results| prism::backends::SearchResultsWithAggs {
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
            })
    };

    let results = match search_result {
        Ok(r) => r,
        Err(prism::Error::InvalidQuery(msg)) => {
            tracing::warn!(
                collection = %collections[0],
                "es-compat: query failed to parse, returning empty results: {}",
                msg
            );
            prism::backends::SearchResultsWithAggs {
                results: vec![],
                total: 0,
                aggregations: std::collections::HashMap::new(),
            }
        }
        Err(e) => return Err(e.into()),
    };

    let took_ms = start.elapsed().as_millis() as u64;

    // Map to ES response format, applying any `_source` include/exclude filter.
    let response = ResponseMapper::map_search_results_with_source(
        &index_name,
        results,
        took_ms,
        request.source.as_ref(),
        request.track_total_hits.as_ref(),
    );

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

/// Encode a collection name into a PIT id (base64).
fn encode_pit_id(collection: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.encode(collection)
}

/// Decode a PIT id back into a collection name.
fn decode_pit_collection(pit_id: &str) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.decode(pit_id).ok().and_then(|b| String::from_utf8(b).ok())
}

/// POST /{index}/_pit?keep_alive=10m - Create a Point-In-Time.
///
/// Prism implements a stateless pseudo-PIT: the returned id is just the
/// collection name base64-encoded. Since Prism is single-node and the Kibana
/// saved-object migration has no concurrent writers, ordinary searches are
/// already consistent enough for the migration's paginated reads.
pub async fn create_pit_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<serde_json::Value>, EsCompatError> {
    let collections = state.manager.list_collections();
    if !collections.contains(&index) {
        return Err(EsCompatError::IndexNotFound(index));
    }
    let pit_id = encode_pit_id(&index);
    Ok(Json(serde_json::json!({
        "id": pit_id,
        "_shards": {"successful": 1, "failed": 0, "total": 1}
    })))
}

/// DELETE /_pit - Close a Point-In-Time. Stateless in Prism, so this is a no-op.
pub async fn delete_pit_handler(
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, EsCompatError> {
    // Accept {"id": "..."} or array of ids; we don't track PITs, so just ack.
    let n = match &body {
        serde_json::Value::Array(a) => a.len(),
        serde_json::Value::Object(_) => 1,
        _ => 0,
    };
    Ok(Json(serde_json::json!({
        "succeeded": true,
        "num_freed": n
    })))
}
