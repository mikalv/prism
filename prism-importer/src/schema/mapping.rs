use serde::Deserialize;
use std::collections::HashMap;
use crate::error::Result;
use super::types::{SourceSchema, SourceField, SourceFieldType};

/// Elasticsearch mapping response structure
#[derive(Debug, Deserialize)]
pub struct EsMappingResponse {
    #[serde(flatten)]
    pub indices: HashMap<String, EsIndexMapping>,
}

#[derive(Debug, Deserialize)]
pub struct EsIndexMapping {
    pub mappings: EsMappings,
}

#[derive(Debug, Deserialize)]
pub struct EsMappings {
    #[serde(default)]
    pub properties: HashMap<String, EsFieldMapping>,
}

#[derive(Debug, Deserialize)]
pub struct EsFieldMapping {
    #[serde(rename = "type")]
    pub field_type: Option<String>,
    #[serde(default)]
    pub properties: Option<HashMap<String, EsFieldMapping>>,
    pub dims: Option<usize>,
    pub index: Option<bool>,
}

/// Convert Elasticsearch mapping to SourceSchema
pub fn convert_es_mapping(index_name: &str, mapping: &EsMappings) -> Result<SourceSchema> {
    let mut fields = Vec::new();

    for (name, prop) in &mapping.properties {
        let field = convert_field(name, prop)?;
        fields.push(field);
    }

    Ok(SourceSchema {
        name: index_name.to_string(),
        fields,
    })
}

fn convert_field(name: &str, prop: &EsFieldMapping) -> Result<SourceField> {
    let es_type = prop.field_type.as_deref().unwrap_or("object");

    let (field_type, vector_dims) = match es_type {
        "text" | "match_only_text" => (SourceFieldType::Text, None),
        "keyword" | "constant_keyword" | "wildcard" => (SourceFieldType::Keyword, None),
        "long" | "integer" | "short" | "byte" => (SourceFieldType::I64, None),
        "float" | "double" | "half_float" | "scaled_float" => (SourceFieldType::F64, None),
        "boolean" => (SourceFieldType::Bool, None),
        "date" | "date_nanos" => (SourceFieldType::Date, None),
        "dense_vector" => (SourceFieldType::Vector, prop.dims),
        "object" | "nested" | "flattened" => (SourceFieldType::Json, None),
        other => {
            tracing::warn!("Unknown ES type '{}' for field '{}', mapping to text", other, name);
            (SourceFieldType::Unknown(other.to_string()), None)
        }
    };

    let indexed = prop.index.unwrap_or(true);

    Ok(SourceField {
        name: name.to_string(),
        field_type,
        indexed,
        vector_dims,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_basic_types() {
        let mapping_json = json!({
            "properties": {
                "title": { "type": "text" },
                "category": { "type": "keyword" },
                "price": { "type": "float" },
                "count": { "type": "integer" },
                "active": { "type": "boolean" },
                "created": { "type": "date" }
            }
        });

        let mapping: EsMappings = serde_json::from_value(mapping_json).unwrap();
        let schema = convert_es_mapping("products", &mapping).unwrap();

        assert_eq!(schema.name, "products");
        assert_eq!(schema.fields.len(), 6);

        let title = schema.fields.iter().find(|f| f.name == "title").unwrap();
        assert_eq!(title.field_type, SourceFieldType::Text);

        let price = schema.fields.iter().find(|f| f.name == "price").unwrap();
        assert_eq!(price.field_type, SourceFieldType::F64);
    }

    #[test]
    fn test_convert_vector_field() {
        let mapping_json = json!({
            "properties": {
                "embedding": { "type": "dense_vector", "dims": 384 }
            }
        });

        let mapping: EsMappings = serde_json::from_value(mapping_json).unwrap();
        let schema = convert_es_mapping("docs", &mapping).unwrap();

        let embedding = schema.fields.iter().find(|f| f.name == "embedding").unwrap();
        assert_eq!(embedding.field_type, SourceFieldType::Vector);
        assert_eq!(embedding.vector_dims, Some(384));
    }

    #[test]
    fn test_unknown_type_falls_back() {
        let mapping_json = json!({
            "properties": {
                "geo": { "type": "geo_point" }
            }
        });

        let mapping: EsMappings = serde_json::from_value(mapping_json).unwrap();
        let schema = convert_es_mapping("places", &mapping).unwrap();

        let geo = schema.fields.iter().find(|f| f.name == "geo").unwrap();
        assert!(matches!(geo.field_type, SourceFieldType::Unknown(_)));
    }
}
