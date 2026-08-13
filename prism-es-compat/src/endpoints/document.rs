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
use prism::schema::{
    types::SystemFieldsConfig,
    CollectionSchema, Backends, TextBackendConfig, IndexingConfig, QuotaConfig,
};
use prism::storage::StorageConfig;
use serde::{Deserialize, Serialize};
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

/// POST /_elastic/{index}/_create/{id} - Create a document with explicit ID.
///
/// This is the endpoint the official ES client's `client.create()` maps to
/// (NOT `_doc/:id`), and Kibana's saved-objects repository uses it for every
/// create operation (task scheduling, index patterns, config, ...). Without it,
/// all saved-object writes 404, which cascades into "Error scheduling task ...
/// Not Found" across nearly every plugin. `manager.index()` follows aliases,
/// so `require_alias=true` targets like `.kibana_task_manager` route to the
/// backing concrete index transparently. op_type=create's 409-on-exists is not
/// enforced (treated as upsert); Kibana uses unique ids and retries idempotently.
pub async fn create_doc_handler(
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

    // Auto-create index if it doesn't exist
    if state.manager.get_schema(&index).is_none() {
        let schema = CollectionSchema {
            collection: index.clone(),
            description: Some("Auto-created by Kibana".to_string()),
            backends: Backends {
                text: Some(TextBackendConfig {
                    fields: vec![],
                    bm25_k1: None,
                    bm25_b: None,
                }),
                vector: None,
                graph: None,
            },
            indexing: IndexingConfig::default(),
            quota: QuotaConfig::default(),
            embedding_generation: None,
            facets: None,
            boosting: None,
            storage: StorageConfig::default(),
            system_fields: SystemFieldsConfig::default(),
            hybrid: None,
            replication: None,
            reranking: None,
            ilm_policy: None,
        };
        state.manager.add_collection(schema).await?;
    }

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

/// HEAD /{index} - Check if index exists
pub async fn head_index_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<impl IntoResponse, EsCompatError> {
    let collections = state.manager.list_collections();
    // Support comma-separated multi-index requests
    let indices: Vec<&str> = index.split(',').collect();
    if indices.len() == 1 {
        if collections.contains(&index) {
            return Ok(StatusCode::OK);
        } else {
            return Ok(StatusCode::NOT_FOUND);
        }
    }
    // Multi-index: all must exist for 200
    for idx in indices {
        if !collections.contains(&idx.to_string()) {
            return Ok(StatusCode::NOT_FOUND);
        }
    }
    Ok(StatusCode::OK)
}

/// ES index info response
#[derive(Debug, Clone, Serialize)]
pub struct EsIndexInfo {
    pub aliases: std::collections::HashMap<String, serde_json::Value>,
    pub mappings: serde_json::Value,
    pub settings: serde_json::Value,
}

/// ES create-index request body (mappings + settings)
#[derive(Debug, Deserialize, Default)]
struct EsCreateIndexBody {
    #[serde(default)]
    mappings: Option<serde_json::Value>,
    #[serde(default)]
    settings: Option<serde_json::Value>,
}

/// Convert an ES field type string to a Prism FieldType.
/// Unknown/complex types fall back to "text" (safest for search).
fn es_type_to_prism(es_type: &str) -> prism::schema::types::FieldType {
    use prism::schema::types::FieldType;
    match es_type {
        "text" => FieldType::Text,
        "keyword" | "ip" | "version" => FieldType::String,
        "long" | "integer" | "short" | "byte" => FieldType::I64,
        "unsigned_long" => FieldType::U64,
        "double" | "float" | "scaled_float" | "half_float" => FieldType::F64,
        "boolean" => FieldType::Bool,
        "date" => FieldType::Date,
        "binary" => FieldType::Bytes,
        // nested, object, alias, geo_*, completion, etc → store as text so nothing is lost
        _ => FieldType::Text,
    }
}

/// Walk ES mapping properties and build Prism TextFields.
/// Multi-fields (e.g. {"type":"text","fields":{"raw":{"type":"keyword"}}})
/// are flattened: the main field plus one sub-field per entry.
fn build_fields_from_mappings(
    mappings: &serde_json::Value,
) -> Vec<prism::schema::types::TextField> {
    use prism::schema::types::{FieldType, TextField};

    let mut fields = Vec::new();
    let empty_map = serde_json::Map::new();
    let props_obj = mappings.get("properties").and_then(|p| p.as_object()).unwrap_or(&empty_map);
    let props: Vec<(&String, &serde_json::Value)> = props_obj.iter().collect();

    for (name, spec) in props {
        let spec_obj = match spec.as_object() {
            Some(o) => o,
            None => continue,
        };
        let es_type = spec_obj
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("text");

        // If "type" is missing but "properties" exists → object/nested: recurse with prefix.
        if es_type == "object" || es_type == "nested" || (es_type == "text" && spec_obj.contains_key("properties")) {
            if let Some(nested) = spec_obj.get("properties").and_then(|p| p.as_object()) {
                for (sub_name, sub_spec) in nested {
                    let sub_type = sub_spec
                        .as_object()
                        .and_then(|o| o.get("type"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("text");
                    fields.push(TextField {
                        name: format!("{}.{}", name, sub_name),
                        field_type: es_type_to_prism(sub_type),
                        stored: true,
                        indexed: matches!(
                            es_type_to_prism(sub_type),
                            FieldType::Text | FieldType::String
                        ),
                        tokenizer: None,
                        tokenizer_options: None,
                    });
                }
                continue;
            }
        }

        let ptype = es_type_to_prism(es_type);
        let is_indexable = matches!(ptype, FieldType::Text | FieldType::String);
        fields.push(TextField {
            name: name.clone(),
            field_type: ptype,
            stored: true,
            indexed: is_indexable,
            tokenizer: None,
            tokenizer_options: None,
        });

        // Flatten multi-fields ("fields": {"raw": {"type":"keyword"}})
        if let Some(sub_fields) = spec_obj.get("fields").and_then(|f| f.as_object()) {
            for (sub_name, sub_spec) in sub_fields {
                let sub_type = sub_spec
                    .as_object()
                    .and_then(|o| o.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("keyword");
                fields.push(TextField {
                    name: format!("{}.{}", name, sub_name),
                    field_type: es_type_to_prism(sub_type),
                    stored: true,
                    indexed: true,
                    tokenizer: None,
                    tokenizer_options: None,
                });
            }
        }
    }

    // Always ensure a catch-all "message" text field if nothing was produced
    if fields.is_empty() {
        fields.push(TextField {
            name: "message".to_string(),
            field_type: FieldType::Text,
            stored: true,
            indexed: true,
            tokenizer: None,
            tokenizer_options: None,
        });
    }
    fields
}

/// PUT /{index} - Create an index (collection) with optional mappings/settings.
/// Returns {"acknowledged": true, "shards_acknowledged": true, "index": "<name>"}.
pub async fn put_index_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
    body: Option<axum::Json<serde_json::Value>>,
) -> Result<axum::Json<serde_json::Value>, EsCompatError> {
    // Only handle a single index name per PUT (ES multi-index PUT isn't standard).
    if index.contains(',') {
        return Err(EsCompatError::InvalidRequestBody(format!(
            "Cannot create multiple indices in one request: {index}"
        )));
    }

    // If the collection already exists, ES returns a 400 resource_already_exists.
    let collections = state.manager.list_collections();
    if collections.contains(&index) {
        return Err(EsCompatError::ResourceAlreadyExists(format!(
            "index [{0}/{0}] already exists",
            index
        )));
    }

    // Parse the optional body for mappings.
    let raw_body = body.map(|b| b.0).unwrap_or(serde_json::json!({}));
    let parsed: EsCreateIndexBody = if raw_body.is_null() || raw_body.as_object().map(|o| o.is_empty()).unwrap_or(true) {
        EsCreateIndexBody::default()
    } else {
        serde_json::from_value(raw_body).map_err(|e| EsCompatError::InvalidRequestBody(e.to_string()))?
    };

    // Build fields from ES mappings (or a sensible default).
    let fields = if let Some(ref mappings) = parsed.mappings {
        build_fields_from_mappings(mappings)
    } else {
        vec![prism::schema::types::TextField {
            name: "message".to_string(),
            field_type: prism::schema::types::FieldType::Text,
            stored: true,
            indexed: true,
            tokenizer: None,
            tokenizer_options: None,
        }]
    };

    // Build the Prism CollectionSchema.
    let schema = prism::schema::types::CollectionSchema {
        collection: index.clone(),
        description: Some("Auto-created from Elasticsearch create-index request".to_string()),
        backends: prism::schema::types::Backends {
            text: Some(prism::schema::types::TextBackendConfig {
                fields,
                bm25_k1: None,
                bm25_b: None,
            }),
            vector: None,
            graph: None,
        },
        indexing: Default::default(),
        quota: Default::default(),
        embedding_generation: None,
        facets: None,
        boosting: None,
        storage: Default::default(),
        system_fields: Default::default(),
        hybrid: None,
        replication: None,
        reranking: None,
        ilm_policy: None,
    };

    // Persist schema to disk so it survives restarts, then register the collection.
    state
        .manager
        .persist_schema(&schema)
        .map_err(|e| EsCompatError::Internal(format!("persist_schema failed: {e}")))?;
    state
        .manager
        .add_collection(schema)
        .await
        .map_err(|e| EsCompatError::Internal(format!("add_collection failed: {e}")))?;

    tracing::info!(index = %index, "Created collection from ES PUT /{{index}} request");

    Ok(axum::Json(serde_json::json!({
        "acknowledged": true,
        "shards_acknowledged": true,
        "index": index,
    })))
}

/// GET /{index} - Get index info (mappings, settings, aliases)
pub async fn get_index_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<std::collections::HashMap<String, EsIndexInfo>>, EsCompatError> {
    let ignore_unavailable = params.get("ignore_unavailable").map(|v| v == "true").unwrap_or(false);
    let collections = state.manager.list_collections();
    // Support comma-separated multi-index requests
    let indices: Vec<&str> = index.split(',').collect();
    let mut result = std::collections::HashMap::new();
    let mut found_any = false;

    for idx in indices {
        if !collections.contains(&idx.to_string()) {
            if ignore_unavailable {
                continue;
            } else {
                return Err(EsCompatError::IndexNotFound(index.clone()));
            }
        }

        let schema = state.manager.get_schema(idx)
            .ok_or_else(|| EsCompatError::Internal("schema not found".to_string()))?;

        // Build mappings from schema fields
        let mut properties = std::collections::HashMap::new();
        for field in schema.backends.text.as_ref().map(|t| &t.fields).unwrap_or(&vec![]) {
            let field_type = match field.field_type {
                prism::schema::types::FieldType::Text => "text",
                prism::schema::types::FieldType::String => "keyword",
                prism::schema::types::FieldType::I64 | prism::schema::types::FieldType::U64 => "long",
                prism::schema::types::FieldType::F64 => "double",
                prism::schema::types::FieldType::Bool => "boolean",
                prism::schema::types::FieldType::Date => "date",
                prism::schema::types::FieldType::Bytes => "binary",
            };
            let mut field_map = serde_json::json!({"type": field_type});
            if field.field_type == prism::schema::types::FieldType::Text {
                field_map["fields"] = serde_json::json!({"raw": {"type": "keyword"}});
            }
            properties.insert(field.name.clone(), field_map);
        }

        let mappings = serde_json::json!({
            "properties": properties
        });

        let settings = serde_json::json!({
            "index": {
                "number_of_shards": "1",
                "number_of_replicas": "0",
                "creation_date": chrono::Utc::now().timestamp_millis(),
                "uuid": uuid::Uuid::new_v4().to_string(),
                "version": {
                    "created": format!("{}-{}", env!("CARGO_PKG_VERSION"), std::env::var("PRISM_ES_VERSION").unwrap_or_else(|_| "7.17.0".to_string())),
                },
            }
        });

        result.insert(idx.to_string(), EsIndexInfo {
            aliases: std::collections::HashMap::new(),
            mappings,
            settings,
        });
        found_any = true;
    }

    if !found_any && !result.is_empty() {
        return Err(EsCompatError::IndexNotFound(index.clone()));
    }

    Ok(Json(result))
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
        vector: None,
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
        sort: Vec::new(),
        exists_fields: Vec::new(),
        not_exists_fields: Vec::new(),
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
