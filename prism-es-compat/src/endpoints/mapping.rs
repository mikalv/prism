//! ES-compatible _mapping endpoint

use crate::endpoints::search::EsCompatState;
use crate::error::EsCompatError;
use crate::response::{EsFieldMapping, EsIndexMapping, EsMappingResponse, EsMappings};
use axum::extract::{Path, State};
use axum::Json;
use prism::schema::FieldType;
use std::collections::HashMap;

/// GET /_elastic/{index}/_mapping - Get index mapping
pub async fn mapping_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<EsMappingResponse>, EsCompatError> {
    // Expand index pattern (sync)
    let collections = state
        .manager
        .expand_collection_patterns(&[index.clone()]);

    if collections.is_empty() {
        return Err(EsCompatError::IndexNotFound(index));
    }

    let mut indices = HashMap::new();

    for collection in collections {
        // Get schema (sync)
        let schema = state
            .manager
            .get_schema(&collection)
            .ok_or_else(|| EsCompatError::IndexNotFound(collection.clone()))?;

        let mut properties = HashMap::new();

        // Map text fields
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

                // Add multi-field for text (keyword sub-field)
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

                // Add format for date fields
                if matches!(field.field_type, FieldType::Date) {
                    mapping.format = Some("strict_date_optional_time||epoch_millis".to_string());
                }

                properties.insert(field.name.clone(), mapping);
            }
        }

        // Map vector field (single embedding_field from VectorBackendConfig)
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

        indices.insert(
            collection,
            EsIndexMapping {
                mappings: EsMappings { properties },
            },
        );
    }

    Ok(Json(EsMappingResponse { indices }))
}
