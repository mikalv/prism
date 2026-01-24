//! Utilities for extracting typed field values from Tantivy documents

use std::collections::HashMap;
use tantivy::schema::{OwnedValue, Schema};
use tantivy::{DateTime, TantivyDocument};

/// Extract a string field value from a document
pub fn extract_field_value(doc: &TantivyDocument, field: tantivy::schema::Field) -> Option<String> {
    doc.get_first(field).and_then(|v| match v {
        OwnedValue::Str(s) => Some(s.clone()),
        _ => None,
    })
}

/// Extract a timestamp field from a document
pub fn extract_timestamp(doc: &TantivyDocument, field: tantivy::schema::Field) -> Option<DateTime> {
    doc.get_first(field).and_then(|v| match v {
        OwnedValue::Date(d) => Some(*d),
        OwnedValue::I64(ts) => Some(DateTime::from_timestamp_secs(*ts)),
        OwnedValue::U64(ts) => Some(DateTime::from_timestamp_secs(*ts as i64)),
        _ => None,
    })
}

/// Convert a Tantivy document to a HashMap of JSON values
pub fn convert_doc_to_map(
    doc: &TantivyDocument,
    _schema: &Schema,
    field_map: &HashMap<String, tantivy::schema::Field>,
) -> HashMap<String, serde_json::Value> {
    let mut result = HashMap::new();

    for (field_name, field) in field_map {
        if let Some(value) = doc.get_first(*field) {
            let json_value = match value {
                OwnedValue::Str(s) => serde_json::Value::String(s.clone()),
                OwnedValue::U64(n) => serde_json::Value::Number((*n).into()),
                OwnedValue::I64(n) => serde_json::Value::Number((*n).into()),
                OwnedValue::F64(n) => {
                    if let Some(num) = serde_json::Number::from_f64(*n) {
                        serde_json::Value::Number(num)
                    } else {
                        continue;
                    }
                }
                OwnedValue::Bool(b) => serde_json::Value::Bool(*b),
                OwnedValue::Date(d) => {
                    // Convert DateTime to RFC3339 string
                    serde_json::Value::String(
                        d.into_utc()
                            .format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_else(|_| d.into_utc().to_string()),
                    )
                }
                _ => continue,
            };
            result.insert(field_name.clone(), json_value);
        }
    }

    result
}

/// Extract context fields (project_id, session_id, etc.) for boosting
pub fn extract_context_fields(
    doc: &TantivyDocument,
    field_map: &HashMap<String, tantivy::schema::Field>,
    context_field_names: &[&str],
) -> HashMap<String, String> {
    let mut result = HashMap::new();

    for field_name in context_field_names {
        if let Some(field) = field_map.get(*field_name) {
            if let Some(value) = extract_field_value(doc, *field) {
                result.insert(field_name.to_string(), value);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::schema::{Schema, STORED, STRING, TEXT};
    use tantivy::TantivyDocument;

    fn test_schema() -> (Schema, HashMap<String, tantivy::schema::Field>) {
        let mut schema_builder = Schema::builder();
        let title = schema_builder.add_text_field("title", TEXT | STORED);
        let project_id = schema_builder.add_text_field("project_id", STRING | STORED);
        let schema = schema_builder.build();

        let mut field_map = HashMap::new();
        field_map.insert("title".to_string(), title);
        field_map.insert("project_id".to_string(), project_id);

        (schema, field_map)
    }

    #[test]
    fn test_extract_field_value() {
        let (_schema, field_map) = test_schema();
        let title_field = field_map.get("title").unwrap();

        let mut doc = TantivyDocument::new();
        doc.add_text(*title_field, "Hello World");

        let value = extract_field_value(&doc, *title_field);
        assert_eq!(value, Some("Hello World".to_string()));
    }

    #[test]
    fn test_convert_doc_to_map() {
        let (schema, field_map) = test_schema();
        let title_field = field_map.get("title").unwrap();
        let project_field = field_map.get("project_id").unwrap();

        let mut doc = TantivyDocument::new();
        doc.add_text(*title_field, "Test Title");
        doc.add_text(*project_field, "proj-123");

        let map = convert_doc_to_map(&doc, &schema, &field_map);

        assert_eq!(
            map.get("title"),
            Some(&serde_json::Value::String("Test Title".to_string()))
        );
        assert_eq!(
            map.get("project_id"),
            Some(&serde_json::Value::String("proj-123".to_string()))
        );
    }

    #[test]
    fn test_extract_context_fields() {
        let (_schema, field_map) = test_schema();
        let project_field = field_map.get("project_id").unwrap();

        let mut doc = TantivyDocument::new();
        doc.add_text(*project_field, "proj-456");

        let context = extract_context_fields(&doc, &field_map, &["project_id", "session_id"]);

        assert_eq!(context.get("project_id"), Some(&"proj-456".to_string()));
        assert!(context.get("session_id").is_none()); // Not in doc
    }
}
