//! ES-compatible _bulk endpoint

use crate::endpoints::search::EsCompatState;
use crate::error::EsCompatError;
use crate::query::{BulkAction, BulkActionMeta};
use crate::response::{BulkItemResponse, BulkItemResult, EsBulkResponse, EsError, ShardStats};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::Json;
use prism::backends::Document;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;
use tracing::warn;
use uuid::Uuid;

/// POST /_elastic/_bulk - Bulk indexing
/// POST /_elastic/{index}/_bulk - Bulk indexing with default index
pub async fn bulk_handler(
    State(state): State<EsCompatState>,
    default_index: Option<Path<String>>,
    body: Bytes,
) -> Result<Json<EsBulkResponse>, EsCompatError> {
    let start = Instant::now();
    let default_index = default_index.map(|p| p.0);

    // Parse NDJSON bulk body
    let actions = parse_bulk_body(&body, default_index.as_deref())?;

    let mut items = Vec::with_capacity(actions.len());
    let mut has_errors = false;

    // Group by index for batch processing
    let mut by_index: HashMap<String, Vec<(String, Document)>> = HashMap::new();
    let mut delete_by_index: HashMap<String, Vec<String>> = HashMap::new();

    for action in actions {
        match action {
            BulkAction::Index { index, id, doc } | BulkAction::Create { index, id, doc } => {
                let doc_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                let fields = match doc {
                    Value::Object(obj) => obj.into_iter().collect(),
                    _ => {
                        items.push(BulkItemResponse {
                            index: Some(BulkItemResult {
                                index: index.clone(),
                                id: doc_id,
                                version: 1,
                                result: "error".to_string(),
                                shards: ShardStats::default(),
                                status: 400,
                                error: Some(EsError {
                                    error_type: "mapper_parsing_exception".to_string(),
                                    reason: "Document must be an object".to_string(),
                                }),
                            }),
                            create: None,
                            delete: None,
                        });
                        has_errors = true;
                        continue;
                    }
                };

                by_index
                    .entry(index)
                    .or_default()
                    .push((doc_id, Document { id: String::new(), fields }));
            }
            BulkAction::Delete { index, id } => {
                delete_by_index.entry(index).or_default().push(id);
            }
        }
    }

    // Process index/create actions
    for (index, docs) in by_index {
        // Check if collection exists (sync)
        let collections = state
            .manager
            .expand_collection_patterns(&[index.clone()]);

        if collections.is_empty() {
            // Collection doesn't exist - report errors
            for (doc_id, _) in docs {
                items.push(BulkItemResponse {
                    index: Some(BulkItemResult {
                        index: index.clone(),
                        id: doc_id,
                        version: 1,
                        result: "error".to_string(),
                        shards: ShardStats::default(),
                        status: 404,
                        error: Some(EsError {
                            error_type: "index_not_found_exception".to_string(),
                            reason: format!("no such index [{}]", index),
                        }),
                    }),
                    create: None,
                    delete: None,
                });
                has_errors = true;
            }
            continue;
        }

        let target_index = &collections[0];

        // Prepare documents with IDs
        let prism_docs: Vec<Document> = docs
            .iter()
            .map(|(id, doc)| Document {
                id: id.clone(),
                fields: doc.fields.clone(),
            })
            .collect();

        let doc_ids: Vec<String> = docs.iter().map(|(id, _)| id.clone()).collect();

        // Index documents
        match state.manager.index(target_index, prism_docs).await {
            Ok(_) => {
                for doc_id in doc_ids {
                    items.push(BulkItemResponse {
                        index: Some(BulkItemResult {
                            index: target_index.clone(),
                            id: doc_id,
                            version: 1,
                            result: "created".to_string(),
                            shards: ShardStats::default(),
                            status: 201,
                            error: None,
                        }),
                        create: None,
                        delete: None,
                    });
                }
            }
            Err(e) => {
                warn!("bulk index error: {}", e);
                for doc_id in doc_ids {
                    items.push(BulkItemResponse {
                        index: Some(BulkItemResult {
                            index: target_index.clone(),
                            id: doc_id,
                            version: 1,
                            result: "error".to_string(),
                            shards: ShardStats::default(),
                            status: 500,
                            error: Some(EsError {
                                error_type: "mapper_exception".to_string(),
                                reason: e.to_string(),
                            }),
                        }),
                        create: None,
                        delete: None,
                    });
                    has_errors = true;
                }
            }
        }
    }

    // Process delete actions
    for (index, ids) in delete_by_index {
        let collections = state
            .manager
            .expand_collection_patterns(&[index.clone()]);

        if collections.is_empty() {
            for id in ids {
                items.push(BulkItemResponse {
                    index: None,
                    create: None,
                    delete: Some(BulkItemResult {
                        index: index.clone(),
                        id,
                        version: 1,
                        result: "not_found".to_string(),
                        shards: ShardStats::default(),
                        status: 404,
                        error: None,
                    }),
                });
            }
            continue;
        }

        let target_index = &collections[0];

        match state.manager.delete(target_index, ids.clone()).await {
            Ok(_) => {
                for id in ids {
                    items.push(BulkItemResponse {
                        index: None,
                        create: None,
                        delete: Some(BulkItemResult {
                            index: target_index.clone(),
                            id,
                            version: 1,
                            result: "deleted".to_string(),
                            shards: ShardStats::default(),
                            status: 200,
                            error: None,
                        }),
                    });
                }
            }
            Err(e) => {
                warn!("bulk delete error: {}", e);
                for id in ids {
                    items.push(BulkItemResponse {
                        index: None,
                        create: None,
                        delete: Some(BulkItemResult {
                            index: target_index.clone(),
                            id,
                            version: 1,
                            result: "error".to_string(),
                            shards: ShardStats::default(),
                            status: 500,
                            error: Some(EsError {
                                error_type: "exception".to_string(),
                                reason: e.to_string(),
                            }),
                        }),
                    });
                    has_errors = true;
                }
            }
        }
    }

    let took_ms = start.elapsed().as_millis() as u64;

    Ok(Json(EsBulkResponse {
        took: took_ms,
        errors: has_errors,
        items,
    }))
}

/// Parse NDJSON bulk request body
fn parse_bulk_body(
    body: &Bytes,
    default_index: Option<&str>,
) -> Result<Vec<BulkAction>, EsCompatError> {
    let text = std::str::from_utf8(body)
        .map_err(|e| EsCompatError::InvalidRequestBody(e.to_string()))?;

    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();

    let mut actions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let meta: BulkActionMeta = serde_json::from_str(lines[i])
            .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid action: {}", e)))?;

        if let Some(index_meta) = meta.index {
            let index = index_meta
                .index
                .or_else(|| default_index.map(String::from))
                .ok_or_else(|| EsCompatError::MissingField("_index".to_string()))?;

            i += 1;
            if i >= lines.len() {
                return Err(EsCompatError::InvalidRequestBody(
                    "Missing document body".to_string(),
                ));
            }

            let doc: Value = serde_json::from_str(lines[i])
                .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid doc: {}", e)))?;

            actions.push(BulkAction::Index {
                index,
                id: index_meta.id,
                doc,
            });
        } else if let Some(create_meta) = meta.create {
            let index = create_meta
                .index
                .or_else(|| default_index.map(String::from))
                .ok_or_else(|| EsCompatError::MissingField("_index".to_string()))?;

            i += 1;
            if i >= lines.len() {
                return Err(EsCompatError::InvalidRequestBody(
                    "Missing document body".to_string(),
                ));
            }

            let doc: Value = serde_json::from_str(lines[i])
                .map_err(|e| EsCompatError::InvalidRequestBody(format!("Invalid doc: {}", e)))?;

            actions.push(BulkAction::Create {
                index,
                id: create_meta.id,
                doc,
            });
        } else if let Some(delete_meta) = meta.delete {
            let index = delete_meta
                .index
                .or_else(|| default_index.map(String::from))
                .ok_or_else(|| EsCompatError::MissingField("_index".to_string()))?;

            let id = delete_meta
                .id
                .ok_or_else(|| EsCompatError::MissingField("_id".to_string()))?;

            actions.push(BulkAction::Delete { index, id });
        } else if meta.update.is_some() {
            // Skip update - not supported in v1
            i += 1; // Skip the document body too
            warn!("Update action not supported, skipping");
        }

        i += 1;
    }

    Ok(actions)
}
