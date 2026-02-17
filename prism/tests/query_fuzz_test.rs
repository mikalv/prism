//! Fuzz and edge-case tests for Prism's query parser.
//!
//! Tantivy's QueryParser has surprising behaviour that can cause panics or
//! crashes if not guarded:
//!   - `env:prod` is interpreted as field:value syntax (crashes if field
//!     doesn't exist)
//!   - Multi-word queries default to OR semantics
//!   - Special chars like `()[]{}` can cause parse errors
//!
//! These tests verify that **no query string**, no matter how malformed, can
//! cause a panic.  Returning an `Err` is perfectly fine; unwinding is not.

use prism::backends::{Document, Query, TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use proptest::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `Query` with sensible defaults for testing.
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

/// Spin up a temporary collection with a handful of indexed documents.
///
/// The schema has four fields:
///   - `title`    (text, stored, indexed)
///   - `body`     (text, stored, indexed)
///   - `category` (string, stored, indexed)
///   - `level`    (string, stored, indexed)
async fn setup_fuzz_environment() -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");

    std::fs::create_dir_all(&schemas_dir).expect("Failed to create schemas dir");

    std::fs::write(
        schemas_dir.join("fuzz-collection.yaml"),
        r#"
collection: fuzz-collection
backends:
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
      - name: body
        type: text
        stored: true
        indexed: true
      - name: category
        type: string
        stored: true
        indexed: true
      - name: level
        type: string
        stored: true
        indexed: true
"#,
    )
    .expect("Failed to write schema");

    let text_backend =
        Arc::new(TextBackend::new(&data_dir).expect("Failed to create text backend"));
    let vector_backend =
        Arc::new(VectorBackend::new(&data_dir).expect("Failed to create vector backend"));
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend, None)
            .expect("Failed to create collection manager"),
    );
    manager
        .initialize()
        .await
        .expect("Failed to initialize manager");

    // Index ~100 documents so there is real data to match against.
    let categories = ["tech", "science", "art", "music", "sports"];
    let levels = ["info", "warning", "error", "debug", "trace"];
    let titles = [
        "Rust programming guide",
        "Python for data science",
        "JavaScript async patterns",
        "Go concurrency models",
        "C++ memory management",
        "TypeScript best practices",
        "Java enterprise patterns",
        "Kotlin coroutines deep dive",
        "Swift UI development",
        "Ruby on Rails tutorial",
    ];
    let bodies = [
        "Learn how to write safe and efficient systems code using ownership and borrowing.",
        "Data analysis and machine learning with popular frameworks and libraries.",
        "Promises, async/await, and event loops explained with practical examples.",
        "Goroutines, channels, and select statements for concurrent programming.",
        "Smart pointers, RAII, and move semantics for deterministic resource management.",
        "Strong typing, interfaces, and generics for large scale application development.",
        "Dependency injection, service layers, and repository patterns for enterprise code.",
        "Structured concurrency, flow, and suspend functions for asynchronous work.",
        "Declarative UI framework with state management and combine integration.",
        "Model-view-controller architecture with active record and convention over configuration.",
    ];

    let mut docs = Vec::with_capacity(100);
    for i in 0..100 {
        docs.push(Document {
            id: format!("doc-{}", i),
            fields: HashMap::from([
                ("title".to_string(), json!(titles[i % titles.len()])),
                ("body".to_string(), json!(bodies[i % bodies.len()])),
                (
                    "category".to_string(),
                    json!(categories[i % categories.len()]),
                ),
                ("level".to_string(), json!(levels[i % levels.len()])),
            ]),
        });
    }

    manager
        .index("fuzz-collection", docs)
        .await
        .expect("Failed to index documents");

    (temp, manager)
}

// ---------------------------------------------------------------------------
// Proptest-based fuzz tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// 1. Random ASCII strings (1..200 chars) must never panic.
    #[test]
    fn test_random_ascii_queries_no_panic(query in "[[:ascii:]]{1,200}") {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup_fuzz_environment().await;
            let q = make_query(&query, 10);
            // The search must complete without panicking.
            // Returning Ok or Err are both acceptable outcomes.
            let _result = manager.search("fuzz-collection", q, None).await;
        });
    }

    /// 2. Random unicode strings must never panic.
    #[test]
    fn test_random_unicode_queries_no_panic(query in "\\PC{1,200}") {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup_fuzz_environment().await;
            let q = make_query(&query, 10);
            let _result = manager.search("fuzz-collection", q, None).await;
        });
    }

    /// 3. Strings composed entirely of special characters must not panic.
    #[test]
    fn test_special_chars_no_panic(
        query in prop::collection::vec(
            prop::sample::select(vec![
                '(', ')', '[', ']', '{', '}', ':', ';', '"', '\'',
                '*', '?', '~', '^', '+', '-', '/', '\\', '!', '@',
                '#', '$', '%', '&', '|', '<', '>',
            ]),
            1..50,
        )
    ) {
        let query_str: String = query.into_iter().collect();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup_fuzz_environment().await;
            let q = make_query(&query_str, 10);
            let _result = manager.search("fuzz-collection", q, None).await;
        });
    }

    /// 4. `field:value` patterns with random field names must not panic.
    ///    Tantivy treats `word:word` as a field-scoped query and will fail if the
    ///    field does not exist.  Our backend should catch this gracefully.
    #[test]
    fn test_field_colon_syntax(
        field in "[a-zA-Z_][a-zA-Z0-9_]{0,20}",
        value in "[a-zA-Z0-9]{1,30}",
    ) {
        let query_str = format!("{}:{}", field, value);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_temp, manager) = setup_fuzz_environment().await;
            let q = make_query(&query_str, 10);
            let _result = manager.search("fuzz-collection", q, None).await;
        });
    }
}

// ---------------------------------------------------------------------------
// Deterministic edge-case tests
// ---------------------------------------------------------------------------

/// 5. Empty query string.
#[tokio::test]
async fn test_empty_query() {
    let (_temp, manager) = setup_fuzz_environment().await;
    let q = make_query("", 10);
    let result = manager.search("fuzz-collection", q, None).await;
    // Must not panic. Either Ok (0 results or all docs) or Err is fine.
    match &result {
        Ok(r) => {
            // Zero results or some results are both acceptable.
            assert!(r.total <= 100, "should not exceed indexed doc count");
        }
        Err(_) => {
            // Returning an error for an empty query is acceptable.
        }
    }
}

/// 6. Very long query string (10,000 characters).
#[tokio::test]
async fn test_very_long_query() {
    let (_temp, manager) = setup_fuzz_environment().await;
    let long_query = "rust ".repeat(2000); // 10,000 chars
    let q = make_query(&long_query, 10);
    let _result = manager.search("fuzz-collection", q, None).await;
    // Must not panic. Performance is not under test here.
}

/// 7. Query consisting entirely of special characters.
#[tokio::test]
async fn test_only_special_chars() {
    let (_temp, manager) = setup_fuzz_environment().await;
    let q = make_query("*&^%$#@!", 10);
    let _result = manager.search("fuzz-collection", q, None).await;
}

/// 8. Unbalanced parentheses.
#[tokio::test]
async fn test_unbalanced_parens() {
    let (_temp, manager) = setup_fuzz_environment().await;

    let cases = vec![
        "((foo AND bar)",
        "foo)",
        "(((",
        ")))",
        "(((a OR b)",
        "a AND (b OR c))",
        "((()))",
    ];

    for case in cases {
        let q = make_query(case, 10);
        let _result = manager.search("fuzz-collection", q, None).await;
        // Must not panic for any of these.
    }
}

/// 9. Unbalanced quotes.
#[tokio::test]
async fn test_unbalanced_quotes() {
    let (_temp, manager) = setup_fuzz_environment().await;

    let cases = vec![
        r#""hello world"#,
        r#"he said "hi"#,
        r#""""#,
        r#"'single quote"#,
        r#"it's a test"#,
        r#""nested "quotes" here"#,
    ];

    for case in cases {
        let q = make_query(case, 10);
        let _result = manager.search("fuzz-collection", q, None).await;
    }
}

/// 10. SQL injection attempt.
#[tokio::test]
async fn test_sql_injection_attempt() {
    let (_temp, manager) = setup_fuzz_environment().await;

    let injection_strings = vec![
        "'; DROP TABLE users; --",
        "1 OR 1=1",
        "\" OR \"\"=\"",
        "1; SELECT * FROM information_schema.tables --",
        "UNION SELECT username, password FROM users",
        "' OR '1'='1",
    ];

    for injection in injection_strings {
        let q = make_query(injection, 10);
        let result = manager.search("fuzz-collection", q, None).await;
        // Must not panic. Should return Ok with 0 results or Err.
        if let Ok(r) = result {
            // SQL injection should not magically reveal all documents.
            // This is a search engine, not SQL, so any result count is
            // technically fine, but we just verify no panic.
            let _ = r.total;
        }
    }
}

/// 11. Field:value syntax with a non-existent field.
///     This was an actual production bug where Tantivy's QueryParser panics
///     on `nonexistent_field:value`.
#[tokio::test]
async fn test_field_value_nonexistent_field() {
    let (_temp, manager) = setup_fuzz_environment().await;

    let cases = vec![
        "nonexistent_field:value",
        "env:prod",
        "host:localhost",
        "status:200",
        "unknown_field:\"some phrase\"",
        "a:b:c:d",
        ":value_only",
        "field:",
    ];

    for case in cases {
        let q = make_query(case, 10);
        let _result = manager.search("fuzz-collection", q, None).await;
        // Must not panic. Returning Err is the expected behaviour for
        // non-existent fields.
    }
}

/// 12. Wildcard patterns.
#[tokio::test]
async fn test_wildcard_patterns() {
    let (_temp, manager) = setup_fuzz_environment().await;

    let cases = vec![
        "te*",
        "*est",
        "t?st",
        "*",
        "**",
        "???",
        "rust*",
        "*python*",
        "?",
        "a*b*c",
    ];

    for case in cases {
        let q = make_query(case, 10);
        let _result = manager.search("fuzz-collection", q, None).await;
    }
}

/// 13. Bare boolean operators.
#[tokio::test]
async fn test_boolean_operators() {
    let (_temp, manager) = setup_fuzz_environment().await;

    let cases = vec![
        "AND",
        "OR",
        "NOT",
        "AND OR NOT",
        "AND AND AND",
        "OR OR OR",
        "NOT NOT NOT",
        "AND OR",
        "NOT AND",
    ];

    for case in cases {
        let q = make_query(case, 10);
        let _result = manager.search("fuzz-collection", q, None).await;
    }
}

/// 14. Nested boolean expressions.
#[tokio::test]
async fn test_nested_boolean() {
    let (_temp, manager) = setup_fuzz_environment().await;

    let cases = vec![
        "(a AND b) OR (c AND d)",
        "NOT (a OR b)",
        "((a AND b) OR c) AND (d OR (e AND f))",
        "(((nested)))",
        "a AND (b OR (c AND (d OR e)))",
        "(a OR b) AND NOT c",
        "+required -excluded optional",
    ];

    for case in cases {
        let q = make_query(case, 10);
        let _result = manager.search("fuzz-collection", q, None).await;
    }
}

/// 15. Null bytes embedded in the query string.
#[tokio::test]
async fn test_null_bytes() {
    let (_temp, manager) = setup_fuzz_environment().await;

    let cases = vec![
        "hello\0world",
        "\0",
        "\0\0\0",
        "rust\0programming",
        "before\0after\0end",
        "\0leading",
        "trailing\0",
    ];

    for case in cases {
        let q = make_query(case, 10);
        let _result = manager.search("fuzz-collection", q, None).await;
    }
}
