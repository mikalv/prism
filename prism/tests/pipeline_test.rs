use prism::backends::Document;
use prism::pipeline::processors::*;
use prism::pipeline::Processor;
use serde_json::Value;

fn make_doc(fields: Vec<(&str, Value)>) -> Document {
    Document {
        id: "test-1".to_string(),
        fields: fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    }
}

#[test]
fn test_lowercase_processor() {
    let proc = LowercaseProcessor {
        field: "title".to_string(),
    };
    let mut doc = make_doc(vec![("title", Value::String("Hello WORLD".to_string()))]);
    proc.process(&mut doc).unwrap();
    assert_eq!(
        doc.fields["title"],
        Value::String("hello world".to_string())
    );
}

#[test]
fn test_lowercase_missing_field_errors() {
    let proc = LowercaseProcessor {
        field: "missing".to_string(),
    };
    let mut doc = make_doc(vec![("title", Value::String("Hello".to_string()))]);
    assert!(proc.process(&mut doc).is_err());
}

#[test]
fn test_lowercase_non_string_errors() {
    let proc = LowercaseProcessor {
        field: "count".to_string(),
    };
    let mut doc = make_doc(vec![("count", Value::Number(42.into()))]);
    assert!(proc.process(&mut doc).is_err());
}

#[test]
fn test_html_strip_processor() {
    let proc = HtmlStripProcessor {
        field: "content".to_string(),
    };
    let mut doc = make_doc(vec![(
        "content",
        Value::String("<p>Hello <b>world</b></p>".to_string()),
    )]);
    proc.process(&mut doc).unwrap();
    assert_eq!(
        doc.fields["content"],
        Value::String("Hello world".to_string())
    );
}

#[test]
fn test_set_processor_static_value() {
    let proc = SetProcessor {
        field: "status".to_string(),
        value: "indexed".to_string(),
    };
    let mut doc = make_doc(vec![]);
    proc.process(&mut doc).unwrap();
    assert_eq!(doc.fields["status"], Value::String("indexed".to_string()));
}

#[test]
fn test_set_processor_now_template() {
    let proc = SetProcessor {
        field: "ts".to_string(),
        value: "{{_now}}".to_string(),
    };
    let mut doc = make_doc(vec![]);
    proc.process(&mut doc).unwrap();
    // Should be an ISO8601 timestamp string
    let val = doc.fields["ts"].as_str().unwrap();
    assert!(
        val.contains("T"),
        "Expected ISO8601 timestamp, got: {}",
        val
    );
}

#[test]
fn test_remove_processor() {
    let proc = RemoveProcessor {
        field: "secret".to_string(),
    };
    let mut doc = make_doc(vec![
        ("title", Value::String("hi".to_string())),
        ("secret", Value::String("password".to_string())),
    ]);
    proc.process(&mut doc).unwrap();
    assert!(!doc.fields.contains_key("secret"));
    assert!(doc.fields.contains_key("title"));
}

#[test]
fn test_remove_missing_field_is_ok() {
    let proc = RemoveProcessor {
        field: "nonexistent".to_string(),
    };
    let mut doc = make_doc(vec![]);
    // Removing a missing field should not error
    assert!(proc.process(&mut doc).is_ok());
}

#[test]
fn test_rename_processor() {
    let proc = RenameProcessor {
        from: "old".to_string(),
        to: "new".to_string(),
    };
    let mut doc = make_doc(vec![("old", Value::String("value".to_string()))]);
    proc.process(&mut doc).unwrap();
    assert!(!doc.fields.contains_key("old"));
    assert_eq!(doc.fields["new"], Value::String("value".to_string()));
}

#[test]
fn test_rename_missing_field_errors() {
    let proc = RenameProcessor {
        from: "missing".to_string(),
        to: "new".to_string(),
    };
    let mut doc = make_doc(vec![]);
    assert!(proc.process(&mut doc).is_err());
}

use prism::pipeline::registry::PipelineRegistry;
use tempfile::TempDir;

#[test]
fn test_load_pipeline_from_yaml() {
    let tmp = TempDir::new().unwrap();
    let yaml = r#"
name: normalize
description: Normalize text fields
processors:
  - lowercase:
      field: title
  - html_strip:
      field: content
  - set:
      field: indexed_at
      value: "{{_now}}"
  - remove:
      field: _internal
  - rename:
      from: old
      to: new
"#;
    std::fs::write(tmp.path().join("normalize.yaml"), yaml).unwrap();

    let registry = PipelineRegistry::load(tmp.path()).unwrap();
    assert!(registry.get("normalize").is_some());
    assert!(registry.get("nonexistent").is_none());

    let pipeline = registry.get("normalize").unwrap();
    assert_eq!(pipeline.name, "normalize");
    assert_eq!(pipeline.processors.len(), 5);
}

#[test]
fn test_pipeline_processes_document() {
    let tmp = TempDir::new().unwrap();
    let yaml = r#"
name: test
description: Test pipeline
processors:
  - lowercase:
      field: title
  - remove:
      field: secret
"#;
    std::fs::write(tmp.path().join("test.yaml"), yaml).unwrap();

    let registry = PipelineRegistry::load(tmp.path()).unwrap();
    let pipeline = registry.get("test").unwrap();

    let mut doc = make_doc(vec![
        ("title", Value::String("HELLO".to_string())),
        ("secret", Value::String("password".to_string())),
    ]);

    pipeline.process(&mut doc).unwrap();
    assert_eq!(doc.fields["title"], Value::String("hello".to_string()));
    assert!(!doc.fields.contains_key("secret"));
}

#[test]
fn test_empty_pipeline_dir() {
    let tmp = TempDir::new().unwrap();
    let registry = PipelineRegistry::load(tmp.path()).unwrap();
    assert!(registry.get("anything").is_none());
}

#[test]
fn test_load_nonexistent_dir() {
    let registry = PipelineRegistry::load(std::path::Path::new("/nonexistent/path"));
    // Should succeed with empty registry, not crash
    assert!(registry.is_ok());
    assert!(registry.unwrap().get("anything").is_none());
}
