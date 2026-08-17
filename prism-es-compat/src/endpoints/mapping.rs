//! ES-compatible _mapping endpoint

use crate::endpoints::search::EsCompatState;
use crate::error::EsCompatError;
use crate::response::{EsFieldMapping, EsIndexMapping, EsMappingResponse, EsMappings};
use axum::extract::{Path, State};
use axum::Json;
use prism::schema::FieldType;
use std::collections::HashMap;
use serde_json::{json, Value};

/// GET /{index}/_mapping - Get index mapping
pub async fn mapping_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<EsMappingResponse>, EsCompatError> {
    // Support comma-separated multi-index requests
    let indices: Vec<&str> = index.split(',').collect();
    let mut all_indices = Vec::new();
    if indices.len() == 1 {
        all_indices = state.manager.expand_collection_patterns(std::slice::from_ref(&index));
    } else {
        for idx in indices {
            let expanded = state.manager.expand_collection_patterns(&[idx.to_string()]);
            all_indices.extend(expanded);
        }
    }
    if all_indices.is_empty() {
        return Err(EsCompatError::IndexNotFound(index));
    }

    let mut result_indices = HashMap::new();

    for collection in all_indices {
        let schema = state.manager.get_schema(&collection)
            .ok_or_else(|| EsCompatError::IndexNotFound(collection.clone()))?;

        let mut properties = HashMap::new();

        if let Some(text_config) = &schema.backends.text {
            for field in &text_config.fields {
                let field_type = match field.field_type {
                    FieldType::Text => "text",
                    FieldType::String => "keyword",
                    FieldType::I64 | FieldType::U64 => "long",
                    FieldType::F64 => "double",
                    FieldType::Bool => "boolean",
                    FieldType::Date => "date",
                    FieldType::Bytes => "binary",
                };

                let mut mapping = EsFieldMapping {
                    field_type: field_type.to_string(),
                    fields: None,
                    format: None,
                };

                if matches!(field.field_type, FieldType::Text) {
                    let mut sub_fields = HashMap::new();
                    sub_fields.insert(
                        "keyword".to_string(),
                        EsFieldMapping {
                            field_type: "keyword".to_string(),
                            fields: None,
                            format: None,
                        },
                    );
                    mapping.fields = Some(sub_fields);
                }

                if matches!(field.field_type, FieldType::Date) {
                    mapping.format = Some("strict_date_optional_time||epoch_millis".to_string());
                }

                properties.insert(field.name.clone(), mapping);
            }
        }

        if let Some(vector_config) = &schema.backends.vector {
            properties.insert(
                vector_config.embedding_field.clone(),
                EsFieldMapping {
                    field_type: "dense_vector".to_string(),
                    fields: None,
                    format: None,
                },
            );
        }

        result_indices.insert(
            collection,
            EsIndexMapping {
                mappings: EsMappings { properties },
            },
        );
    }

    Ok(Json(EsMappingResponse { indices: result_indices }))
}

/// PUT /{index}/_mapping - Update index mapping.
///
/// Prism collections have a fixed schema defined at creation, so mapping
/// updates are accepted as a no-op and acknowledged. This satisfies ES clients
/// (notably the Kibana saved-objects migration, which PUTs mappings during
/// index setup at the UPDATE_TARGET_MAPPINGS_PROPERTIES step).
pub async fn put_mapping_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, EsCompatError> {
    // Kibana sometimes issues PUT /{index}/_mapping with an empty body
    // (merge-patch no-op); ES accepts it, so must we.
    let _ = body;
    let collections = state.manager.expand_collection_patterns(std::slice::from_ref(&index));
    if collections.is_empty() {
        return Err(EsCompatError::IndexNotFound(index));
    }
    tracing::debug!(
        index = %index,
        "PUT /_mapping acknowledged (no-op; prism collections have fixed schemas)"
    );
    Ok(Json(json!({ "acknowledged": true })))
}
