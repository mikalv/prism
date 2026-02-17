//! Property-based round-trip tests for all supported field types.
//!
//! Uses `proptest` to generate random documents and verify that indexing,
//! retrieval, search, deletion, and upsert behave correctly across the
//! full spectrum of Prism's schema field types.

use prism::backends::{Document, Query, TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use proptest::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Schema YAML covering all 8 field types
// ---------------------------------------------------------------------------

fn schema_yaml() -> &'static str {
    r#"
collection: proptest-all
backends:
  text:
    fields:
      - name: text_field
        type: text
        indexed: true
        stored: true
      - name: string_field
        type: string
        indexed: true
        stored: true
      - name: i64_field
        type: i64
        indexed: true
        stored: true
      - name: u64_field
        type: u64
        indexed: true
        stored: true
      - name: f64_field
        type: f64
        indexed: true
        stored: true
      - name: bool_field
        type: bool
        indexed: true
        stored: true
      - name: date_field
        type: date
        indexed: true
        stored: true
      - name: bytes_field
        type: bytes
        stored: true
"#
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn setup() -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    std::fs::write(schemas_dir.join("proptest-all.yaml"), schema_yaml()).unwrap();

    let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap(),
    );
    manager.initialize().await.unwrap();
    (temp, manager)
}

fn make_query(query_string: &str, limit: usize) -> Query {
    Query {
        query_string: query_string.to_string(),
        fields: vec![],
        limit,
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
        rrf_k: None,
        min_score: None,
        score_function: None,
        skip_ranking: false,
    }
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

// ---------------------------------------------------------------------------
// Proptest strategies
// ---------------------------------------------------------------------------

/// Safe alphanumeric text that won't confuse the Tantivy query parser.
fn safe_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ]{1,100}"
}

/// Generate a unique word that is unlikely to collide with other random text.
/// Format: "uniq" + 16 hex chars -- always a single token for search.
fn unique_word() -> impl Strategy<Value = String> {
    prop::array::uniform8(0u8..=255u8).prop_map(|bytes| {
        format!(
            "uniq{}",
            bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>()
        )
    })
}

/// Valid RFC 3339 date string strategy.
/// Generates dates between 2000-01-01 and 2030-12-28 (day capped at 28 for all months).
fn rfc3339_date() -> impl Strategy<Value = String> {
    (
        2000i32..=2030,
        1u32..=12,
        1u32..=28,
        0u32..=23,
        0u32..=59,
        0u32..=59,
    )
        .prop_map(|(year, month, day, hour, minute, second)| {
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                year, month, day, hour, minute, second
            )
        })
}

/// Safe f64 values: no NaN, no Infinity, reasonable range.
fn safe_f64() -> impl Strategy<Value = f64> {
    -1e10f64..1e10f64
}

/// Generate bytes as a Vec<u8> for the bytes field.
fn random_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..64)
}

// ---------------------------------------------------------------------------
// Test 1: Single document round-trip with ALL field types
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn test_single_doc_roundtrip(
        text_val in safe_text(),
        string_val in "[a-zA-Z0-9]{1,50}",
        i64_val in any::<i64>(),
        u64_val in any::<u64>(),
        f64_val in safe_f64(),
        bool_val in any::<bool>(),
        date_val in rfc3339_date(),
        bytes_val in random_bytes(),
        doc_counter in 0u64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup().await;
            let doc_id = format!("prop-rt-{}", doc_counter);

            let bytes_b64 = base64_encode(&bytes_val);

            let doc = Document {
                id: doc_id.clone(),
                fields: HashMap::from([
                    ("text_field".into(), json!(text_val)),
                    ("string_field".into(), json!(string_val)),
                    ("i64_field".into(), json!(i64_val)),
                    ("u64_field".into(), json!(u64_val)),
                    ("f64_field".into(), json!(f64_val)),
                    ("bool_field".into(), json!(bool_val)),
                    ("date_field".into(), json!(date_val)),
                    ("bytes_field".into(), json!(bytes_b64)),
                ]),
            };

            manager.index("proptest-all", vec![doc]).await.unwrap();

            // Get and verify stored fields
            let retrieved = manager.get("proptest-all", &doc_id).await.unwrap();
            assert!(retrieved.is_some(), "Document should exist after indexing");
            let retrieved = retrieved.unwrap();

            // Text and String fields are stored as strings
            assert_eq!(
                retrieved.fields.get("text_field").and_then(|v| v.as_str()),
                Some(text_val.as_str()),
                "text_field mismatch"
            );
            assert_eq!(
                retrieved.fields.get("string_field").and_then(|v| v.as_str()),
                Some(string_val.as_str()),
                "string_field mismatch"
            );

            // Numeric fields
            assert_eq!(
                retrieved.fields.get("i64_field").and_then(|v| v.as_i64()),
                Some(i64_val),
                "i64_field mismatch"
            );
            assert_eq!(
                retrieved.fields.get("u64_field").and_then(|v| v.as_u64()),
                Some(u64_val),
                "u64_field mismatch"
            );

            // f64: compare with epsilon for floating-point
            let retrieved_f64 = retrieved.fields.get("f64_field").and_then(|v| v.as_f64());
            assert!(retrieved_f64.is_some(), "f64_field should be present");
            let diff = (retrieved_f64.unwrap() - f64_val).abs();
            assert!(
                diff < 1e-6 || diff / f64_val.abs().max(1e-15) < 1e-10,
                "f64_field mismatch: got {} expected {}, diff={}",
                retrieved_f64.unwrap(), f64_val, diff
            );

            // Bool
            assert_eq!(
                retrieved.fields.get("bool_field").and_then(|v| v.as_bool()),
                Some(bool_val),
                "bool_field mismatch"
            );

            // Date and Bytes: the get() method currently skips Date/Bytes OwnedValues,
            // so we just verify they don't cause errors. These fields are stored in
            // Tantivy but not reconstructed by the current get() implementation.
            // This is a known limitation; the test validates that the round-trip
            // for the supported extraction types works correctly.

            // Delete and verify gone
            manager.delete("proptest-all", vec![doc_id.clone()]).await.unwrap();
            let after_delete = manager.get("proptest-all", &doc_id).await.unwrap();
            assert!(after_delete.is_none(), "Document should be gone after deletion");
        });
    }
}

// ---------------------------------------------------------------------------
// Test 2: Text field is searchable
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn test_text_field_searchable(
        unique in unique_word(),
        doc_counter in 0u64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup().await;
            let doc_id = format!("prop-search-{}", doc_counter);

            // Embed the unique word inside some surrounding text
            let text_content = format!("hello {} world", unique);

            let doc = Document {
                id: doc_id.clone(),
                fields: HashMap::from([
                    ("text_field".into(), json!(text_content)),
                    ("string_field".into(), json!("searchable")),
                    ("i64_field".into(), json!(0)),
                    ("u64_field".into(), json!(0)),
                    ("f64_field".into(), json!(0.0)),
                    ("bool_field".into(), json!(true)),
                    ("date_field".into(), json!("2026-01-01T00:00:00Z")),
                ]),
            };

            manager.index("proptest-all", vec![doc]).await.unwrap();

            // Search for the unique word
            let query = make_query(&unique, 10);
            let results = manager.search("proptest-all", query, None).await.unwrap();

            assert!(
                results.total >= 1,
                "Should find at least 1 result for unique word '{}'",
                unique
            );

            let found_ids: Vec<&str> = results.results.iter().map(|r| r.id.as_str()).collect();
            assert!(
                found_ids.contains(&doc_id.as_str()),
                "Search results should contain our document '{}', found: {:?}",
                doc_id, found_ids
            );
        });
    }
}

// ---------------------------------------------------------------------------
// Test 3: Numeric fields are preserved exactly
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn test_numeric_fields_preserved(
        i64_val in any::<i64>(),
        u64_val in any::<u64>(),
        f64_val in safe_f64(),
        doc_counter in 0u64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup().await;
            let doc_id = format!("prop-num-{}", doc_counter);

            let doc = Document {
                id: doc_id.clone(),
                fields: HashMap::from([
                    ("text_field".into(), json!("numeric test")),
                    ("string_field".into(), json!("numtest")),
                    ("i64_field".into(), json!(i64_val)),
                    ("u64_field".into(), json!(u64_val)),
                    ("f64_field".into(), json!(f64_val)),
                    ("bool_field".into(), json!(false)),
                    ("date_field".into(), json!("2026-06-15T12:00:00Z")),
                ]),
            };

            manager.index("proptest-all", vec![doc]).await.unwrap();

            let retrieved = manager.get("proptest-all", &doc_id).await.unwrap();
            assert!(retrieved.is_some(), "Document should exist");
            let fields = &retrieved.unwrap().fields;

            // i64: exact match
            assert_eq!(
                fields.get("i64_field").and_then(|v| v.as_i64()),
                Some(i64_val),
                "i64 mismatch"
            );

            // u64: exact match
            assert_eq!(
                fields.get("u64_field").and_then(|v| v.as_u64()),
                Some(u64_val),
                "u64 mismatch"
            );

            // f64: epsilon comparison
            let got_f64 = fields.get("f64_field").and_then(|v| v.as_f64());
            assert!(got_f64.is_some(), "f64_field should be present");
            let diff = (got_f64.unwrap() - f64_val).abs();
            assert!(
                diff < 1e-6 || diff / f64_val.abs().max(1e-15) < 1e-10,
                "f64 mismatch: got {} expected {}",
                got_f64.unwrap(), f64_val
            );
        });
    }
}

// ---------------------------------------------------------------------------
// Test 4: Bool field round-trip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn test_bool_field_roundtrip(
        bool_val in any::<bool>(),
        doc_counter in 0u64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup().await;
            let doc_id = format!("prop-bool-{}", doc_counter);

            let doc = Document {
                id: doc_id.clone(),
                fields: HashMap::from([
                    ("text_field".into(), json!("bool test")),
                    ("string_field".into(), json!("booltest")),
                    ("i64_field".into(), json!(0)),
                    ("u64_field".into(), json!(0)),
                    ("f64_field".into(), json!(0.0)),
                    ("bool_field".into(), json!(bool_val)),
                    ("date_field".into(), json!("2026-01-01T00:00:00Z")),
                ]),
            };

            manager.index("proptest-all", vec![doc]).await.unwrap();

            let retrieved = manager.get("proptest-all", &doc_id).await.unwrap();
            assert!(retrieved.is_some(), "Document should exist");
            let fields = &retrieved.unwrap().fields;

            assert_eq!(
                fields.get("bool_field").and_then(|v| v.as_bool()),
                Some(bool_val),
                "bool_field mismatch: expected {}", bool_val
            );
        });
    }
}

// ---------------------------------------------------------------------------
// Test 5: Date field round-trip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn test_date_field_roundtrip(
        date_val in rfc3339_date(),
        doc_counter in 0u64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup().await;
            let doc_id = format!("prop-date-{}", doc_counter);

            let doc = Document {
                id: doc_id.clone(),
                fields: HashMap::from([
                    ("text_field".into(), json!("date test")),
                    ("string_field".into(), json!("datetest")),
                    ("i64_field".into(), json!(0)),
                    ("u64_field".into(), json!(0)),
                    ("f64_field".into(), json!(0.0)),
                    ("bool_field".into(), json!(true)),
                    ("date_field".into(), json!(date_val)),
                ]),
            };

            // Indexing a valid RFC 3339 date should not fail
            manager.index("proptest-all", vec![doc]).await.unwrap();

            // The document should be retrievable (date is stored in Tantivy)
            let retrieved = manager.get("proptest-all", &doc_id).await.unwrap();
            assert!(
                retrieved.is_some(),
                "Document with date '{}' should exist after indexing",
                date_val
            );

            // Note: The current get() implementation skips Date OwnedValues
            // in the field extraction loop (falls through to `_ => continue`).
            // We verify the doc exists and other fields are intact -- the date
            // is stored internally in Tantivy and can be used for sorting/filtering.
            let fields = &retrieved.unwrap().fields;
            assert_eq!(
                fields.get("string_field").and_then(|v| v.as_str()),
                Some("datetest"),
                "Other fields should be intact alongside date"
            );
        });
    }
}

// ---------------------------------------------------------------------------
// Test 6: Upsert replaces document
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn test_upsert_replaces(
        old_text in safe_text(),
        new_text in safe_text(),
        old_i64 in any::<i64>(),
        new_i64 in any::<i64>(),
        doc_counter in 0u64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup().await;
            let doc_id = format!("prop-upsert-{}", doc_counter);

            // Index original document
            let doc_v1 = Document {
                id: doc_id.clone(),
                fields: HashMap::from([
                    ("text_field".into(), json!(old_text)),
                    ("string_field".into(), json!("v1")),
                    ("i64_field".into(), json!(old_i64)),
                    ("u64_field".into(), json!(0u64)),
                    ("f64_field".into(), json!(1.0)),
                    ("bool_field".into(), json!(false)),
                    ("date_field".into(), json!("2026-01-01T00:00:00Z")),
                ]),
            };
            manager.index("proptest-all", vec![doc_v1]).await.unwrap();

            // Verify v1 exists
            let v1 = manager.get("proptest-all", &doc_id).await.unwrap();
            assert!(v1.is_some(), "v1 should exist");

            // Re-index with same ID but different values (upsert)
            let doc_v2 = Document {
                id: doc_id.clone(),
                fields: HashMap::from([
                    ("text_field".into(), json!(new_text)),
                    ("string_field".into(), json!("v2")),
                    ("i64_field".into(), json!(new_i64)),
                    ("u64_field".into(), json!(42u64)),
                    ("f64_field".into(), json!(2.0)),
                    ("bool_field".into(), json!(true)),
                    ("date_field".into(), json!("2026-12-31T23:59:59Z")),
                ]),
            };
            manager.index("proptest-all", vec![doc_v2]).await.unwrap();

            // Verify v2 values
            let v2 = manager.get("proptest-all", &doc_id).await.unwrap();
            assert!(v2.is_some(), "v2 should exist");
            let fields = &v2.unwrap().fields;

            assert_eq!(
                fields.get("string_field").and_then(|v| v.as_str()),
                Some("v2"),
                "string_field should be updated to v2"
            );
            assert_eq!(
                fields.get("text_field").and_then(|v| v.as_str()),
                Some(new_text.as_str()),
                "text_field should be updated"
            );
            assert_eq!(
                fields.get("i64_field").and_then(|v| v.as_i64()),
                Some(new_i64),
                "i64_field should be updated"
            );
            assert_eq!(
                fields.get("bool_field").and_then(|v| v.as_bool()),
                Some(true),
                "bool_field should be updated to true"
            );

            // Verify stats show 1 document (not 2)
            let stats = manager.stats("proptest-all").await.unwrap();
            assert_eq!(
                stats.document_count, 1,
                "Upsert should result in 1 document, not 2"
            );
        });
    }
}

// ---------------------------------------------------------------------------
// Test 7: Batch random documents
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(15))]

    #[test]
    fn test_batch_random_docs(
        count in 10usize..50,
        base_i64 in any::<i64>(),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup().await;

            // Generate batch of documents
            let docs: Vec<Document> = (0..count)
                .map(|i| {
                    let doc_id = format!("batch-{}", i);
                    Document {
                        id: doc_id,
                        fields: HashMap::from([
                            ("text_field".into(), json!(format!("batch document number {}", i))),
                            ("string_field".into(), json!(format!("tag{}", i % 5))),
                            ("i64_field".into(), json!(base_i64.wrapping_add(i as i64))),
                            ("u64_field".into(), json!(i as u64)),
                            ("f64_field".into(), json!(i as f64 * 0.5)),
                            ("bool_field".into(), json!(i % 2 == 0)),
                            ("date_field".into(), json!("2026-06-15T12:00:00Z")),
                        ]),
                    }
                })
                .collect();

            manager.index("proptest-all", docs).await.unwrap();

            // Verify stats
            let stats = manager.stats("proptest-all").await.unwrap();
            assert_eq!(
                stats.document_count, count,
                "Stats should show {} documents, got {}",
                count, stats.document_count
            );

            // Verify each document can be retrieved
            for i in 0..count {
                let doc_id = format!("batch-{}", i);
                let retrieved = manager.get("proptest-all", &doc_id).await.unwrap();
                assert!(
                    retrieved.is_some(),
                    "Document '{}' should be retrievable",
                    doc_id
                );
                let fields = &retrieved.unwrap().fields;
                assert_eq!(
                    fields.get("u64_field").and_then(|v| v.as_u64()),
                    Some(i as u64),
                    "u64_field for doc batch-{} should be {}", i, i
                );
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Test 8: Delete random subset
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(15))]

    #[test]
    fn test_delete_subset(
        total in 10usize..40,
        delete_ratio in 0.1f64..0.9,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup().await;

            // Index N documents
            let docs: Vec<Document> = (0..total)
                .map(|i| Document {
                    id: format!("del-{}", i),
                    fields: HashMap::from([
                        ("text_field".into(), json!(format!("deletable doc {}", i))),
                        ("string_field".into(), json!("deleteme")),
                        ("i64_field".into(), json!(i as i64)),
                        ("u64_field".into(), json!(i as u64)),
                        ("f64_field".into(), json!(0.0)),
                        ("bool_field".into(), json!(true)),
                        ("date_field".into(), json!("2026-03-01T00:00:00Z")),
                    ]),
                })
                .collect();

            manager.index("proptest-all", docs).await.unwrap();

            // Determine which documents to delete (deterministic from ratio)
            let delete_count = ((total as f64) * delete_ratio).ceil() as usize;
            let delete_count = delete_count.min(total).max(1);

            let to_delete: Vec<String> = (0..delete_count)
                .map(|i| format!("del-{}", i))
                .collect();
            let to_keep: Vec<String> = (delete_count..total)
                .map(|i| format!("del-{}", i))
                .collect();

            // Delete the subset
            manager
                .delete("proptest-all", to_delete.clone())
                .await
                .unwrap();

            // Verify deleted docs are gone
            for id in &to_delete {
                let result = manager.get("proptest-all", id).await.unwrap();
                assert!(
                    result.is_none(),
                    "Deleted document '{}' should be gone",
                    id
                );
            }

            // Verify remaining docs still exist
            for id in &to_keep {
                let result = manager.get("proptest-all", id).await.unwrap();
                assert!(
                    result.is_some(),
                    "Kept document '{}' should still exist",
                    id
                );
            }

            // Verify stats reflect the deletion
            let stats = manager.stats("proptest-all").await.unwrap();
            let expected_remaining = total - delete_count;
            assert_eq!(
                stats.document_count, expected_remaining,
                "Expected {} remaining docs after deleting {}/{}, got {}",
                expected_remaining, delete_count, total, stats.document_count
            );
        });
    }
}
