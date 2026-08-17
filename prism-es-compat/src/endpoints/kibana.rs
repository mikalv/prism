//! Kibana 9.x compatibility endpoints that Kibana's startup and Discover
//! require beyond core CRUD/search:
//!
//! - `GET /{index}/_alias[/{name}]` — alerting/plugin install flows probe the
//!   write alias after creating an index; a missing route 404s and sends the
//!   installer into a create → resource_already_exists error loop.
//! - `POST /{index}/_field_caps` (and `/_field_caps`) — Discover + data-view
//!   creation fetch field capabilities; without it Discover cannot render.
//! - `POST /_security/user/_has_privileges` — allow-all stub (prism's ES-compat
//!   layer has no security model).
//! - `GET /_resolve/index/{pattern}` — data-views and index management.
//! - `PUT /{index}/_settings` — Kibana plugins bump index settings (e.g.
//!   `index.mapping.total_fields.limit`); acknowledge and ignore.
//! - `GET /_inference` — inference-endpoint poller expects a list.
//! - `PUT|GET|DELETE /_ingest/pipeline/{id}` — stored as metadata, acked.
//! - `GET /_stats`, `GET /{index}/_stats` — minimal index stats.

use crate::endpoints::search::EsCompatState;
use crate::error::EsCompatError;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Map, Value};

/// Map a prism field type to the ES field_caps entry.
fn es_type_name(ft: &prism::schema::types::FieldType) -> &'static str {
    use prism::schema::types::FieldType::*;
    match ft {
        Text | String => "text",
        I64 | U64 => "long",
        F64 => "double",
        Bool => "boolean",
        Date => "date",
        Bytes => "binary",
    }
}

/// ES `text` fields are also aggregatable via the `.keyword` sub-field.
fn field_caps_for_schema(schema: &prism::schema::types::CollectionSchema, out: &mut Map<String, Value>) {
    let Some(ref text_cfg) = schema.backends.text else {
        return;
    };
    for f in &text_cfg.fields {
        // ISO-8601 timestamp strings sort lexicographically, so range queries
        // work on the underlying string field; report well-known timestamp
        // field names as ES `date` so Kibana data views can use them as the
        // time field (a `text`/`keyword` time field breaks the time picker
        // and date_histogram aggregations).
        let is_ts_field =
            matches!(f.name.as_str(), "timestamp" | "@timestamp" | "time");
        let es_type = if is_ts_field {
            "date"
        } else {
            es_type_name(&f.field_type)
        };
        let mut variants = Map::new();
        variants.insert(
            es_type.to_string(),
            json!({
                "type": es_type,
                "searchable": true,
                "aggregatable": es_type != "text",
            }),
        );
        if es_type == "text" {
            // ES dynamic mappings expose `field.keyword`; Kibana uses this for
            // sortable/aggregatable columns on text fields.
            variants.insert(
                "keyword".to_string(),
                json!({
                    "type": "keyword",
                    "searchable": true,
                    "aggregatable": true,
                }),
            );
        }
        out.insert(f.name.clone(), Value::Object(variants));
    }
}

/// POST|GET /_field_caps
pub async fn field_caps_handler(
    State(state): State<EsCompatState>,
) -> Result<Json<Value>, EsCompatError> {
    field_caps_impl(state, None).await
}

/// POST|GET /{index}/_field_caps
pub async fn field_caps_index_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<Value>, EsCompatError> {
    field_caps_impl(state, Some(&index)).await
}

async fn field_caps_impl(
    state: EsCompatState,
    index: Option<&str>,
) -> Result<Json<Value>, EsCompatError> {
    let mut fields = Map::new();

    if let Some(patterns) = index {
        let expanded = state
            .manager
            .expand_collection_patterns(patterns.split(',').map(str::to_string).collect::<Vec<_>>().as_slice());
        for c in &expanded {
            if let Some(schema) = state.manager.get_schema(c) {
                field_caps_for_schema(&schema, &mut fields);
            }
        }
    } else {
        for c in state.manager.list_collections() {
            if let Some(schema) = state.manager.get_schema(&c) {
                field_caps_for_schema(&schema, &mut fields);
            }
        }
    }

    Ok(Json(json!({
        "indices": {},
        "fields": Value::Object(fields),
    })))
}

/// GET /{index}/_alias — all aliases of the (pattern of) index/indices.
/// GET /{index}/_alias/{name} — one alias; ES 404s if the index exists but
/// the alias is absent (that's the signal Kibana's installer relies on).
/// GET /_alias — list all aliases across all indices. No path params.
/// Dedicated to the top-level route; `get_alias_handler` requires an index
/// path segment and axum rejects it with a plain-text 500 otherwise.
pub async fn get_alias_all_handler(
    State(state): State<EsCompatState>,
) -> Result<Json<Value>, EsCompatError> {
    let snapshot = state.alias_store.snapshot();
    let mut out = Map::new();
    for (index, aliases) in snapshot {
        let mut m = Map::new();
        for (name, body) in aliases {
            m.insert(name, body);
        }
        if !m.is_empty() {
            out.insert(index, json!({ "aliases": Value::Object(m) }));
        }
    }
    Ok(Json(Value::Object(out)))
}

/// GET /_alias/{name} — all indices carrying alias `{name}`. ES semantics:
/// 404 with `alias_missing_exception` when no index has the alias.
pub async fn get_alias_global_name_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, EsCompatError> {
    let snapshot = state.alias_store.snapshot();
    let mut out = Map::new();
    for (index, aliases) in snapshot {
        if let Some((_, body)) = aliases.iter().find(|(n, _)| n.as_str() == name) {
            out.insert(index, json!({ "aliases": { name.clone(): body.clone() } }));
        }
    }
    if out.is_empty() {
        return Err(EsCompatError::IndexNotFound(format!("alias [{name}] missing")));
    }
    Ok(Json(Value::Object(out)))
}

pub async fn get_alias_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<Value>, EsCompatError> {
    let expanded = state
        .manager
        .expand_collection_patterns(std::slice::from_ref(&index));
    if expanded.is_empty() && !index.contains('*') {
        return Err(EsCompatError::IndexNotFound(index));
    }
    let mut out = Map::new();
    for c in &expanded {
        let aliases = state.alias_store.for_index(c);
        if !aliases.is_empty() {
            out.insert(c.clone(), json!({ "aliases": aliases_to_map(&aliases) }));
        }
    }
    Ok(Json(Value::Object(out)))
}

fn aliases_to_map(aliases: &[(String, Value)]) -> Value {
    let mut m = Map::new();
    for (name, body) in aliases {
        m.insert(name.clone(), body.clone());
    }
    Value::Object(m)
}

pub async fn get_alias_name_handler(
    State(state): State<EsCompatState>,
    Path((index, name)): Path<(String, String)>,
) -> Result<Json<Value>, EsCompatError> {
    let expanded = state
        .manager
        .expand_collection_patterns(std::slice::from_ref(&index));
    if expanded.is_empty() && !index.contains('*') {
        return Err(EsCompatError::IndexNotFound(index));
    }
    let mut out = Map::new();
    for c in &expanded {
        let aliases = state.alias_store.for_index(c);
        if let Some((_, body)) = aliases.iter().find(|(n, _)| n == &name) {
            out.insert(c.clone(), json!({ "aliases": { name.clone(): body.clone() } }));
        }
    }
    if out.is_empty() {
        // ES: alias not found on existing index -> 404
        return Err(EsCompatError::IndexNotFound(format!("alias [{name}] missing")));
    }
    Ok(Json(Value::Object(out)))
}

/// POST /_security/user/_has_privileges — allow-all.
pub async fn has_privileges_handler(
    Json(body): Json<Value>,
) -> Result<Json<Value>, EsCompatError> {
    let mut response = Map::new();
    response.insert("username".to_string(), json!("prism"));
    response.insert("has_all_requested".to_string(), json!(true));
    if let Some(Value::Object(index)) = body.get("index") {
        let mut idx_out = Map::new();
        for (idx, privs) in index {
            if let Value::Object(privs) = privs {
                let mut p_out = Map::new();
                for (p, req) in privs {
                    p_out.insert(p.clone(), json!(req.as_bool().unwrap_or(true)));
                }
                idx_out.insert(idx.clone(), Value::Object(p_out));
            }
        }
        response.insert("index".to_string(), Value::Object(idx_out));
    }
    if let Some(Value::Object(cluster)) = body.get("cluster") {
        let mut c_out = Map::new();
        for (p, req) in cluster {
            c_out.insert(p.clone(), json!(req.as_bool().unwrap_or(true)));
        }
        response.insert("cluster".to_string(), Value::Object(c_out));
    }
    if let Some(Value::Object(apps)) = body.get("application") {
        let mut a_out = Map::new();
        for (app, privs) in apps {
            if let Value::Array(privs) = privs {
                let mut p_out = Map::new();
                for p in privs {
                    if let Some(s) = p.as_str() {
                        p_out.insert(s.to_string(), json!(true));
                    }
                }
                a_out.insert(app.clone(), Value::Object(p_out));
            }
        }
        response.insert("application".to_string(), Value::Object(a_out));
    }
    Ok(Json(Value::Object(response)))
}

/// GET /_resolve/index/{pattern} — indices + aliases + data streams for a pattern.
pub async fn resolve_index_handler(
    State(state): State<EsCompatState>,
    Path(pattern): Path<String>,
) -> Result<Json<Value>, EsCompatError> {
    let expanded = state
        .manager
        .expand_collection_patterns(pattern.split(',').map(str::to_string).collect::<Vec<_>>().as_slice());
    let mut indices = Vec::new();
    for c in &expanded {
        let aliases = state.alias_store.for_index(c);
        let alias_names: Vec<Value> = aliases.iter().map(|(n, _)| json!(n)).collect();
        indices.push(json!({
            "name": c,
            "aliases": alias_names,
            "attributes": ["open"],
        }));
    }
    let aliases: Vec<Value> = state
        .alias_store
        .snapshot()
        .iter()
        .filter(|(a, _)| {
            // include aliases resolvable by the pattern
            expanded
                .iter()
                .any(|i| state.alias_store.for_index(i).iter().any(|(n, _)| n == *a))
        })
        .map(|(a, targets)| {
            json!({
                "name": a,
                "indices": targets.keys().collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(json!({
        "indices": indices,
        "aliases": aliases,
        "data_streams": [],
    })))
}

/// PUT /{index}/_settings — acknowledge (settings are stored as metadata only
/// where meaningful; prism ignores most ES index settings).
pub async fn put_settings_handler(
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, EsCompatError> {
    let _ = body;
    Ok(Json(json!({
        "acknowledged": true,
        "index": { index: {} },
    })))
}

/// GET /_resolve/cluster/{pattern} — Kibana 9.x data-views probe cluster
/// resolution; prism is a single cluster, so return the local one.
pub async fn resolve_cluster_handler() -> Json<Value> {
    Json(json!({
        "local": { "match": true, "clusters": [] },
        "clusters": [],
        "aliases": [],
        "indices": [],
        "data_streams": [],
    }))
}

/// GET /_inference — empty endpoint list.
pub async fn get_inference_handler() -> Json<Value> {
    Json(json!({ "endpoints": [] }))
}

/// PUT /_ingest/pipeline/{id} — ack; DELETE — ack; GET — not-found.
pub async fn put_ingest_pipeline_handler(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "acknowledged": true, "id": id }))
}

pub async fn delete_ingest_pipeline_handler(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "acknowledged": true, "id": id }))
}

/// GET /_stats and GET /{index}/_stats — minimal per-index stats.
pub async fn stats_handler(
    State(state): State<EsCompatState>,
) -> Json<Value> {
    stats_impl(state, None).await
}

pub async fn stats_index_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Json<Value> {
    stats_impl(state, Some(&index)).await
}

async fn stats_impl(state: EsCompatState, index: Option<&str>) -> Json<Value> {
    let names: Vec<String> = match index {
        Some(patterns) => state
            .manager
            .expand_collection_patterns(patterns.split(',').map(str::to_string).collect::<Vec<_>>().as_slice()),
        None => state.manager.list_collections(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let mut shards = Map::new();
    for name in names {
        let doc_count = state
            .manager
            .stats(&name)
            .await
            .map(|s| s.document_count)
            .unwrap_or(0);
        shards.insert(
            name.clone(),
            json!({
                "uuid": name,
                "primaries": {
                    "docs": { "count": doc_count, "deleted": 0 },
                    "shard_stats": { "total_count": 1 },
                },
                "total": {
                    "docs": { "count": doc_count, "deleted": 0 },
                    "shard_stats": { "total_count": 1 },
                },
            }),
        );
    }
    Json(json!({
        "_shards": { "total": shards.len(), "successful": shards.len(), "failed": 0 },
        "_all": { "primaries": {}, "total": {} },
        "indices": Value::Object(shards),
        "timestamp": now,
    }))
}
