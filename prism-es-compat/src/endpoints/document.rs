//! ES-compatible document CRUD endpoints

use crate::error::EsCompatError;
use crate::response::{
    EsCountResponse, EsDeleteResponse, EsGetResponse, EsIndexResponse, ShardStats,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use prism::backends::Document;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use super::search::EsCompatState;

/// GET /_elastic/{index}/_doc/{id} - Get a document by ID
pub async fn get_doc_handler(
    State(state): State<EsCompatState>,
    Path((index, id)): Path<(String, String)>,
) -> Result<Json<EsGetResponse>, EsCompatError> {
    let doc = state.manager.get(&index, &id).await?;

    match doc {
        Some(doc) => Ok(Json(EsGetResponse {
            index,
            id: doc.id,
            version: 1,
            found: true,
            source: Some(doc.fields),
        })),
        None => Ok(Json(EsGetResponse {
            index,
            id,
            version: 1,
            found: false,
            source: None,
        })),
    }
}

/// HEAD /_elastic/{index}/_doc/{id} - Check if document exists
pub async fn head_doc_handler(
    State(state): State<EsCompatState>,
    Path((index, id)): Path<(String, String)>,
) -> Result<impl IntoResponse, EsCompatError> {
    let doc = state.manager.get(&index, &id).await?;

    if doc.is_some() {
        Ok(StatusCode::OK)
    } else {
        Ok(StatusCode::NOT_FOUND)
    }
}

/// POST /_elastic/{index}/_doc - Index a document (auto-generate ID)
pub async fn post_doc_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
    Json(body): Json<HashMap<String, Value>>,
) -> Result<(StatusCode, Json<EsIndexResponse>), EsCompatError> {
    let id = uuid::Uuid::new_v4().to_string();
    let doc = Document {
        id: id.clone(),
        fields: body,
    };

    state.manager.index(&index, vec![doc]).await?;

    Ok((
        StatusCode::CREATED,
        Json(EsIndexResponse {
            index,
            id,
            version: 1,
            result: "created".to_string(),
            shards: ShardStats::default(),
        }),
    ))
}

/// PUT /_elastic/{index}/_doc/{id} - Index a document with explicit ID
pub async fn put_doc_handler(
    State(state): State<EsCompatState>,
    Path((index, id)): Path<(String, String)>,
    Json(body): Json<HashMap<String, Value>>,
) -> Result<(StatusCode, Json<EsIndexResponse>), EsCompatError> {
    let doc = Document {
        id: id.clone(),
        fields: body,
    };

    state.manager.index(&index, vec![doc]).await?;

    Ok((
        StatusCode::CREATED,
        Json(EsIndexResponse {
            index,
            id,
            version: 1,
            result: "created".to_string(),
            shards: ShardStats::default(),
        }),
    ))
}

/// DELETE /_elastic/{index}/_doc/{id} - Delete a document
pub async fn delete_doc_handler(
    State(state): State<EsCompatState>,
    Path((index, id)): Path<(String, String)>,
) -> Result<Json<EsDeleteResponse>, EsCompatError> {
    state.manager.delete(&index, vec![id.clone()]).await?;

    Ok(Json(EsDeleteResponse {
        index,
        id,
        version: 1,
        result: "deleted".to_string(),
        shards: ShardStats::default(),
    }))
}

/// HEAD /_elastic/{index} - Check if index exists
pub async fn head_index_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<impl IntoResponse, EsCompatError> {
    let collections = state.manager.list_collections();
    if collections.contains(&index) {
        Ok(StatusCode::OK)
    } else {
        Ok(StatusCode::NOT_FOUND)
    }
}

/// Query parameters for GET _search
#[derive(Debug, Deserialize, Default)]
pub struct SearchQueryParams {
    pub q: Option<String>,
    pub size: Option<usize>,
    pub from: Option<usize>,
}

/// GET /_elastic/{index}/_search?q=... - Query string search
pub async fn get_search_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
    Query(params): Query<SearchQueryParams>,
) -> Result<Json<crate::response::EsSearchResponse>, EsCompatError> {
    use crate::query::{EsSearchRequest, QueryTranslator};
    use crate::response::ResponseMapper;
    use std::time::Instant;

    let start = Instant::now();

    let collections = state
        .manager
        .expand_collection_patterns(std::slice::from_ref(&index));

    if collections.is_empty() {
        return Err(EsCompatError::IndexNotFound(index));
    }

    // Build an EsSearchRequest from query params
    let request = if let Some(q) = params.q {
        use crate::query::{EsQuery, QueryStringQuery};
        EsSearchRequest {
            query: Some(EsQuery::QueryString(QueryStringQuery {
                query: q,
                default_field: None,
                fields: None,
                default_operator: None,
                analyze_wildcard: None,
            })),
            from: params.from,
            size: params.size,
            ..Default::default()
        }
    } else {
        EsSearchRequest {
            from: params.from,
            size: params.size,
            ..Default::default()
        }
    };

    let default_fields = super::search::get_text_fields(&state.manager, &collections[0]);
    let (query, aggregations) = QueryTranslator::translate(&request, &default_fields)?;

    let results = state
        .manager
        .search_with_aggs(&collections[0], &query, aggregations)
        .await?;

    let took_ms = start.elapsed().as_millis() as u64;
    let response = ResponseMapper::map_search_results(&index, results, took_ms);

    Ok(Json(response))
}

/// GET /_elastic/{index}/_count - Count documents in an index
pub async fn count_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<EsCountResponse>, EsCompatError> {
    let collections = state
        .manager
        .expand_collection_patterns(std::slice::from_ref(&index));

    if collections.is_empty() {
        return Err(EsCompatError::IndexNotFound(index));
    }

    // Use search with size=0 to get total count
    use prism::backends::Query as PrismQuery;

    let query = PrismQuery {
        query_string: "*".to_string(),
        fields: vec![],
        offset: 0,
        limit: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    };

    let results = state
        .manager
        .search_with_aggs(&collections[0], &query, vec![])
        .await?;

    Ok(Json(EsCountResponse {
        count: results.total,
        shards: ShardStats::default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_response_found_serde() {
        let resp = EsGetResponse {
            index: "test".to_string(),
            id: "1".to_string(),
            version: 1,
            found: true,
            source: Some({
                let mut m = HashMap::new();
                m.insert("title".to_string(), Value::String("doc".to_string()));
                m
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"found\":true"));
        assert!(json.contains("\"_source\""));
        assert!(json.contains("\"_index\":\"test\""));
    }

    #[test]
    fn test_get_response_not_found_serde() {
        let resp = EsGetResponse {
            index: "test".to_string(),
            id: "missing".to_string(),
            version: 1,
            found: false,
            source: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"found\":false"));
        assert!(!json.contains("\"_source\""));
    }

    #[test]
    fn test_index_response_serde() {
        let resp = EsIndexResponse {
            index: "test".to_string(),
            id: "1".to_string(),
            version: 1,
            result: "created".to_string(),
            shards: ShardStats::default(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\":\"created\""));
        assert!(json.contains("\"_shards\""));
    }

    #[test]
    fn test_delete_response_serde() {
        let resp = EsDeleteResponse {
            index: "test".to_string(),
            id: "1".to_string(),
            version: 1,
            result: "deleted".to_string(),
            shards: ShardStats::default(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\":\"deleted\""));
    }

    #[test]
    fn test_count_response_serde() {
        let resp = EsCountResponse {
            count: 42,
            shards: ShardStats::default(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"count\":42"));
    }

    #[test]
    fn test_search_query_params_default() {
        let params = SearchQueryParams::default();
        assert!(params.q.is_none());
        assert!(params.size.is_none());
        assert!(params.from.is_none());
    }
}
