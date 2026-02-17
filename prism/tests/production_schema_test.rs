//! Comprehensive integration tests for production-like log schema.
//!
//! Uses a realistic log-ingestion schema with all field types (date, string, text),
//! feeds realistic log data, and verifies every aspect of indexing, searching,
//! retrieval, deletion, and collection lifecycle.

use prism::backends::{Document, Query, TextBackend, VectorBackend};
use prism::collection::CollectionManager;
use prism::schema::CollectionSchema;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

/// A production-like log schema with date, string, and text fields.
fn log_schema_yaml() -> &'static str {
    r#"
collection: test-logs
description: "Integration test application logs"

backends:
  text:
    fields:
      - name: timestamp
        type: date
        stored: true
        indexed: true
      - name: source
        type: string
        stored: true
        indexed: true
      - name: level
        type: string
        stored: true
        indexed: true
      - name: line
        type: text
        stored: true
        indexed: true
      - name: app_id
        type: string
        stored: true
        indexed: true
      - name: machine_id
        type: string
        stored: true
        indexed: true
      - name: org_id
        type: string
        stored: true
        indexed: true
      - name: node_id
        type: string
        stored: true
        indexed: true

boosting:
  recency:
    field: timestamp
    decay_function: exponential
    scale: "1d"
    decay_rate: 0.7
  field_weights:
    line: 2.0
    source: 1.0

reranking:
  type: score_function
  candidates: 200
  score_function: "_score"
"#
}

/// A metrics schema that exercises ALL remaining field types: f64, i64, u64, bool, date, text, string.
fn metrics_schema_yaml() -> &'static str {
    r#"
collection: test-metrics
description: "Integration test metrics with all field types"

backends:
  text:
    fields:
      - name: metric_name
        type: text
        stored: true
        indexed: true
      - name: host
        type: string
        stored: true
        indexed: true
      - name: value_f64
        type: f64
        stored: true
        indexed: true
      - name: value_i64
        type: i64
        stored: true
        indexed: true
      - name: value_u64
        type: u64
        stored: true
        indexed: true
      - name: is_anomaly
        type: bool
        stored: true
        indexed: true
      - name: timestamp
        type: date
        stored: true
        indexed: true
      - name: tags
        type: text
        stored: true
        indexed: true
"#
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

fn make_query_with_fields(query_string: &str, fields: Vec<&str>, limit: usize) -> Query {
    Query {
        query_string: query_string.to_string(),
        fields: fields.into_iter().map(String::from).collect(),
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

fn make_query_with_offset(query_string: &str, limit: usize, offset: usize) -> Query {
    Query {
        query_string: query_string.to_string(),
        fields: vec![],
        limit,
        offset,
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

/// Generate realistic log entries.
fn generate_log_docs(count: usize, prefix: &str) -> Vec<Document> {
    let levels = ["error", "warn", "info", "debug", "trace"];
    let sources = [
        "api-gateway",
        "auth-service",
        "payment-processor",
        "user-service",
        "notification-worker",
    ];
    let apps = ["app-001", "app-002", "app-003"];
    let machines = ["m-east-1", "m-east-2", "m-west-1", "m-west-2"];
    let orgs = ["org-alpha", "org-beta", "org-gamma"];
    let nodes = ["node-a", "node-b", "node-c", "node-d"];

    let log_messages = [
        "Connection pool exhausted, retrying with backoff",
        "Request processed successfully in 42ms",
        "Database query timeout after 30s on users table",
        "Authentication failed for user admin@example.com",
        "Rate limit exceeded for client 10.0.0.1",
        "Deployment completed successfully for version v2.3.1",
        "Memory usage above threshold: 85% utilized",
        "Certificate renewal scheduled for domain api.example.io",
        "Batch import completed: 5000 records processed",
        "WebSocket connection dropped, reconnecting",
        "Cache miss ratio exceeded 40%, invalidating stale entries",
        "Incoming webhook from Stripe: payment_intent.succeeded",
        "gRPC health check passed for all downstream services",
        "Kafka consumer lag detected on topic events.user.signup",
        "TLS handshake failed: certificate expired",
        "Background job enqueued: send_welcome_email (priority: high)",
        "Search query returned 0 results for: 'nonexistent_sku'",
        "API response time P99 spiked to 1200ms",
        "Horizontal pod autoscaler triggered: scaling from 3 to 5",
        "Feature flag 'dark_mode_v2' enabled for 10% of users",
    ];

    (0..count)
        .map(|i| {
            let level = levels[i % levels.len()];
            let source = sources[i % sources.len()];
            let app = apps[i % apps.len()];
            let machine = machines[i % machines.len()];
            let org = orgs[i % orgs.len()];
            let node = nodes[i % nodes.len()];
            let msg = log_messages[i % log_messages.len()];

            let ts = format!("2026-02-17T10:{:02}:{:02}Z", i / 60 % 60, i % 60);

            Document {
                id: format!("{}-{}", prefix, i),
                fields: HashMap::from([
                    ("timestamp".to_string(), json!(ts)),
                    ("source".to_string(), json!(source)),
                    ("level".to_string(), json!(level)),
                    (
                        "line".to_string(),
                        json!(format!("[{}] {} - {}", level.to_uppercase(), source, msg)),
                    ),
                    ("app_id".to_string(), json!(app)),
                    ("machine_id".to_string(), json!(machine)),
                    ("org_id".to_string(), json!(org)),
                    ("node_id".to_string(), json!(node)),
                ]),
            }
        })
        .collect()
}

/// Generate metrics documents that use ALL field types including numeric and bool.
fn generate_metric_docs(count: usize) -> Vec<Document> {
    let metric_names = [
        "cpu_usage_percent",
        "memory_bytes_used",
        "request_latency_ms",
        "disk_iops",
        "network_rx_bytes",
    ];
    let hosts = [
        "host-a.example.io",
        "host-b.example.io",
        "host-c.example.io",
    ];

    (0..count)
        .map(|i| {
            let metric = metric_names[i % metric_names.len()];
            let host = hosts[i % hosts.len()];
            let is_anomaly = i % 7 == 0;
            let ts = format!("2026-02-17T12:{:02}:{:02}Z", i / 60 % 60, i % 60);

            Document {
                id: format!("metric-{}", i),
                fields: HashMap::from([
                    (
                        "metric_name".to_string(),
                        json!(format!("{} on {}", metric, host)),
                    ),
                    ("host".to_string(), json!(host)),
                    ("value_f64".to_string(), json!(42.5 + (i as f64) * 0.1)),
                    ("value_i64".to_string(), json!(-(i as i64) - 100)),
                    ("value_u64".to_string(), json!(i as u64 * 1000)),
                    ("is_anomaly".to_string(), json!(is_anomaly)),
                    ("timestamp".to_string(), json!(ts)),
                    (
                        "tags".to_string(),
                        json!(format!("env:prod region:eu metric:{}", metric)),
                    ),
                ]),
            }
        })
        .collect()
}

/// Setup environment with both collections using schema-on-disk.
async fn setup_environment() -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    std::fs::write(schemas_dir.join("test-logs.yaml"), log_schema_yaml()).unwrap();
    std::fs::write(schemas_dir.join("test-metrics.yaml"), metrics_schema_yaml()).unwrap();

    let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager =
        Arc::new(CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap());
    manager.initialize().await.unwrap();

    (temp, manager)
}

/// Setup with only the log collection.
async fn setup_logs_only() -> (TempDir, Arc<CollectionManager>) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    std::fs::write(schemas_dir.join("test-logs.yaml"), log_schema_yaml()).unwrap();

    let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager =
        Arc::new(CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap());
    manager.initialize().await.unwrap();

    (temp, manager)
}

// ============================================================
// BASIC INDEX + SEARCH
// ============================================================

#[tokio::test]
async fn test_logs_index_and_search_basic() {
    let (_temp, mgr) = setup_logs_only().await;
    let docs = generate_log_docs(100, "basic");
    mgr.index("test-logs", docs).await.unwrap();

    let results = mgr
        .search("test-logs", make_query("connection", 50), None)
        .await
        .unwrap();
    assert!(results.total > 0, "should find docs matching 'connection'");
}

#[tokio::test]
async fn test_logs_index_500_docs() {
    let (_temp, mgr) = setup_logs_only().await;
    let docs = generate_log_docs(500, "bulk");
    mgr.index("test-logs", docs).await.unwrap();

    let stats = mgr.stats("test-logs").await.unwrap();
    assert_eq!(stats.document_count, 500, "all 500 docs should be indexed");
}

// ============================================================
// SEARCH BY TEXT FIELD (line)
// ============================================================

#[tokio::test]
async fn test_logs_search_by_log_message() {
    let (_temp, mgr) = setup_logs_only().await;
    let docs = generate_log_docs(200, "msg");
    mgr.index("test-logs", docs).await.unwrap();

    let cases = [
        ("timeout", true),
        ("authentication", true),
        ("deployment", true),
        ("webhook", true),
        ("certificate", true),
        ("autoscaler", true),
        ("kafka", true),
        ("gRPC", true),
        ("nonexistent_random_gibberish_xyz", false),
    ];

    for (keyword, should_find) in cases {
        let results = mgr
            .search(
                "test-logs",
                make_query_with_fields(keyword, vec!["line"], 50),
                None,
            )
            .await
            .unwrap();
        if should_find {
            assert!(
                results.total > 0,
                "should find docs matching '{}', got 0",
                keyword
            );
        } else {
            assert_eq!(
                results.total, 0,
                "should NOT find docs matching '{}', got {}",
                keyword, results.total
            );
        }
    }
}

// ============================================================
// GET BY ID
// ============================================================

#[tokio::test]
async fn test_logs_get_by_id() {
    let (_temp, mgr) = setup_logs_only().await;
    let docs = generate_log_docs(50, "getid");
    mgr.index("test-logs", docs).await.unwrap();

    for i in [0, 10, 25, 49] {
        let doc = mgr.get("test-logs", &format!("getid-{}", i)).await.unwrap();
        assert!(doc.is_some(), "doc getid-{} should exist", i);
        let doc = doc.unwrap();
        assert_eq!(doc.id, format!("getid-{}", i));
        assert!(
            doc.fields.contains_key("line"),
            "doc should have 'line' field"
        );
        assert!(
            doc.fields.contains_key("source"),
            "doc should have 'source' field"
        );
        assert!(
            doc.fields.contains_key("level"),
            "doc should have 'level' field"
        );
        assert!(
            doc.fields.contains_key("app_id"),
            "doc should have 'app_id' field"
        );
    }

    let missing = mgr.get("test-logs", "no-such-id").await.unwrap();
    assert!(missing.is_none(), "non-existent ID should return None");
}

// ============================================================
// FIELD VALUE INTEGRITY
// ============================================================

#[tokio::test]
async fn test_logs_field_values_roundtrip() {
    let (_temp, mgr) = setup_logs_only().await;

    let doc = Document {
        id: "roundtrip-1".to_string(),
        fields: HashMap::from([
            ("timestamp".to_string(), json!("2026-02-17T15:30:00Z")),
            ("source".to_string(), json!("api-gateway")),
            ("level".to_string(), json!("error")),
            (
                "line".to_string(),
                json!("Connection pool exhausted, retrying with backoff"),
            ),
            ("app_id".to_string(), json!("app-001")),
            ("machine_id".to_string(), json!("m-east-1")),
            ("org_id".to_string(), json!("org-alpha")),
            ("node_id".to_string(), json!("node-a")),
        ]),
    };
    mgr.index("test-logs", vec![doc]).await.unwrap();

    let retrieved = mgr.get("test-logs", "roundtrip-1").await.unwrap().unwrap();
    assert_eq!(retrieved.id, "roundtrip-1");
    assert_eq!(
        retrieved.fields.get("source").and_then(|v| v.as_str()),
        Some("api-gateway")
    );
    assert_eq!(
        retrieved.fields.get("level").and_then(|v| v.as_str()),
        Some("error")
    );
    assert_eq!(
        retrieved.fields.get("line").and_then(|v| v.as_str()),
        Some("Connection pool exhausted, retrying with backoff")
    );
    assert_eq!(
        retrieved.fields.get("app_id").and_then(|v| v.as_str()),
        Some("app-001")
    );
    assert_eq!(
        retrieved.fields.get("machine_id").and_then(|v| v.as_str()),
        Some("m-east-1")
    );
    assert_eq!(
        retrieved.fields.get("org_id").and_then(|v| v.as_str()),
        Some("org-alpha")
    );
    assert_eq!(
        retrieved.fields.get("node_id").and_then(|v| v.as_str()),
        Some("node-a")
    );
}

// ============================================================
// DELETE + SEARCH CONSISTENCY
// ============================================================

#[tokio::test]
async fn test_logs_delete_and_verify() {
    let (_temp, mgr) = setup_logs_only().await;
    let docs = generate_log_docs(100, "del");
    mgr.index("test-logs", docs).await.unwrap();

    let stats = mgr.stats("test-logs").await.unwrap();
    assert_eq!(stats.document_count, 100);

    let ids_to_delete: Vec<String> = (0..50).map(|i| format!("del-{}", i)).collect();
    mgr.delete("test-logs", ids_to_delete).await.unwrap();

    let stats = mgr.stats("test-logs").await.unwrap();
    assert_eq!(
        stats.document_count, 50,
        "50 docs should remain after deleting 50"
    );

    for i in 0..50 {
        let doc = mgr.get("test-logs", &format!("del-{}", i)).await.unwrap();
        assert!(doc.is_none(), "del-{} should be deleted", i);
    }

    for i in 50..100 {
        let doc = mgr.get("test-logs", &format!("del-{}", i)).await.unwrap();
        assert!(doc.is_some(), "del-{} should still exist", i);
    }

    let results = mgr
        .search("test-logs", make_query("connection OR timeout", 100), None)
        .await
        .unwrap();
    for r in &results.results {
        let id_num: usize = r.id.strip_prefix("del-").unwrap().parse().unwrap();
        assert!(
            id_num >= 50,
            "result {} should not be from deleted range",
            r.id
        );
    }
}

// ============================================================
// PAGINATION
// ============================================================

#[tokio::test]
async fn test_logs_pagination() {
    let (_temp, mgr) = setup_logs_only().await;
    let docs: Vec<Document> = (0..100)
        .map(|i| Document {
            id: format!("page-{}", i),
            fields: HashMap::from([
                (
                    "line".to_string(),
                    json!(format!("paginated log entry number {}", i)),
                ),
                ("level".to_string(), json!("info")),
                ("source".to_string(), json!("test")),
                ("app_id".to_string(), json!("app-001")),
                ("machine_id".to_string(), json!("m-east-1")),
                ("org_id".to_string(), json!("org-alpha")),
                ("node_id".to_string(), json!("node-a")),
            ]),
        })
        .collect();
    mgr.index("test-logs", docs).await.unwrap();

    let mut all_ids = Vec::new();
    for page in 0..10 {
        let results = mgr
            .search(
                "test-logs",
                make_query_with_offset("paginated", 10, page * 10),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            results.results.len(),
            10,
            "page {} should have 10 results",
            page
        );
        for r in &results.results {
            all_ids.push(r.id.clone());
        }
    }

    let unique_count = {
        let mut sorted = all_ids.clone();
        sorted.sort();
        sorted.dedup();
        sorted.len()
    };
    assert_eq!(
        unique_count,
        all_ids.len(),
        "pagination should not return duplicate IDs"
    );
}

// ============================================================
// MULTIPLE BATCHES — incremental indexing
// ============================================================

#[tokio::test]
async fn test_logs_incremental_batches() {
    let (_temp, mgr) = setup_logs_only().await;

    for batch in 0..10 {
        let docs = generate_log_docs(50, &format!("batch{}", batch));
        mgr.index("test-logs", docs).await.unwrap();

        let stats = mgr.stats("test-logs").await.unwrap();
        let expected = (batch + 1) * 50;
        assert_eq!(
            stats.document_count, expected,
            "after batch {}, expected {} docs",
            batch, expected
        );

        let results = mgr
            .search("test-logs", make_query("connection OR timeout", 100), None)
            .await
            .unwrap();
        assert!(
            results.total > 0,
            "search should return results after batch {}",
            batch
        );
    }
}

// ============================================================
// CONCURRENT INDEX + SEARCH
// ============================================================

#[tokio::test]
async fn test_logs_concurrent_index_and_search() {
    let (_temp, mgr) = setup_logs_only().await;

    let seed = generate_log_docs(100, "seed");
    mgr.index("test-logs", seed).await.unwrap();

    let mgr1 = mgr.clone();
    let mgr2 = mgr.clone();

    let indexer = tokio::spawn(async move {
        for i in 0..20 {
            let docs = generate_log_docs(10, &format!("concurrent-{}", i));
            mgr1.index("test-logs", docs).await.unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    });

    let searcher = tokio::spawn(async move {
        for _ in 0..50 {
            let result = mgr2
                .search(
                    "test-logs",
                    make_query("connection OR deployment OR webhook", 50),
                    None,
                )
                .await;
            assert!(
                result.is_ok(),
                "concurrent search failed: {:?}",
                result.err()
            );
            assert!(
                result.unwrap().total > 0,
                "concurrent search returned 0 results"
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }
    });

    let (idx_result, search_result) = tokio::join!(indexer, searcher);
    idx_result.unwrap();
    search_result.unwrap();

    let stats = mgr.stats("test-logs").await.unwrap();
    assert_eq!(stats.document_count, 300);
}

// ============================================================
// MULTI-COLLECTION — both collections simultaneously
// ============================================================

#[tokio::test]
async fn test_multi_collection_simultaneous() {
    let (_temp, mgr) = setup_environment().await;

    let logs = generate_log_docs(200, "multi-log");
    let metrics = generate_metric_docs(200);

    mgr.index("test-logs", logs).await.unwrap();
    mgr.index("test-metrics", metrics).await.unwrap();

    let log_stats = mgr.stats("test-logs").await.unwrap();
    let metric_stats = mgr.stats("test-metrics").await.unwrap();
    assert_eq!(log_stats.document_count, 200);
    assert_eq!(metric_stats.document_count, 200);

    let log_results = mgr
        .search("test-logs", make_query("connection", 50), None)
        .await
        .unwrap();
    assert!(log_results.total > 0);

    let metric_results = mgr
        .search("test-metrics", make_query("cpu_usage", 50), None)
        .await
        .unwrap();
    assert!(metric_results.total > 0);

    let ids: Vec<String> = (0..50).map(|i| format!("multi-log-{}", i)).collect();
    mgr.delete("test-logs", ids).await.unwrap();

    let log_stats = mgr.stats("test-logs").await.unwrap();
    let metric_stats = mgr.stats("test-metrics").await.unwrap();
    assert_eq!(log_stats.document_count, 150);
    assert_eq!(
        metric_stats.document_count, 200,
        "metrics should be untouched"
    );
}

// ============================================================
// ALL FIELD TYPES — metrics collection with i64/u64/f64/bool/date
// ============================================================

#[tokio::test]
async fn test_all_field_types_index_and_retrieve() {
    let (_temp, mgr) = setup_environment().await;

    let doc = Document {
        id: "typed-1".to_string(),
        fields: HashMap::from([
            (
                "metric_name".to_string(),
                json!("cpu_usage_percent on host-a"),
            ),
            ("host".to_string(), json!("host-a.example.io")),
            ("value_f64".to_string(), json!(99.95)),
            ("value_i64".to_string(), json!(-42)),
            ("value_u64".to_string(), json!(1234567890_u64)),
            ("is_anomaly".to_string(), json!(true)),
            ("timestamp".to_string(), json!("2026-02-17T12:00:00Z")),
            ("tags".to_string(), json!("env:prod region:eu metric:cpu")),
        ]),
    };
    mgr.index("test-metrics", vec![doc]).await.unwrap();

    let retrieved = mgr.get("test-metrics", "typed-1").await.unwrap().unwrap();
    assert_eq!(retrieved.id, "typed-1");

    assert_eq!(
        retrieved.fields.get("metric_name").and_then(|v| v.as_str()),
        Some("cpu_usage_percent on host-a")
    );
    assert_eq!(
        retrieved.fields.get("host").and_then(|v| v.as_str()),
        Some("host-a.example.io")
    );
    assert_eq!(
        retrieved.fields.get("tags").and_then(|v| v.as_str()),
        Some("env:prod region:eu metric:cpu")
    );

    let f64_val = retrieved.fields.get("value_f64");
    assert!(f64_val.is_some(), "value_f64 should be stored");
    if let Some(v) = f64_val.and_then(|v| v.as_f64()) {
        assert!(
            (v - 99.95).abs() < 0.001,
            "f64 value should be ~99.95, got {}",
            v
        );
    }

    let i64_val = retrieved.fields.get("value_i64");
    assert!(i64_val.is_some(), "value_i64 should be stored");
    if let Some(v) = i64_val.and_then(|v| v.as_i64()) {
        assert_eq!(v, -42, "i64 value should be -42");
    }

    let u64_val = retrieved.fields.get("value_u64");
    assert!(u64_val.is_some(), "value_u64 should be stored");
    if let Some(v) = u64_val.and_then(|v| v.as_u64()) {
        assert_eq!(v, 1234567890, "u64 value should be 1234567890");
    }

    let bool_val = retrieved.fields.get("is_anomaly");
    assert!(bool_val.is_some(), "is_anomaly should be stored");
    if let Some(v) = bool_val.and_then(|v| v.as_bool()) {
        assert!(v, "is_anomaly should be true");
    }
}

#[tokio::test]
async fn test_all_field_types_bulk() {
    let (_temp, mgr) = setup_environment().await;
    let metrics = generate_metric_docs(500);
    mgr.index("test-metrics", metrics).await.unwrap();

    let stats = mgr.stats("test-metrics").await.unwrap();
    assert_eq!(stats.document_count, 500);

    let results = mgr
        .search("test-metrics", make_query("cpu_usage", 100), None)
        .await
        .unwrap();
    assert!(results.total > 0, "should find cpu_usage metrics");

    let results = mgr
        .search(
            "test-metrics",
            make_query_with_fields("prod", vec!["tags"], 100),
            None,
        )
        .await
        .unwrap();
    assert!(results.total > 0, "should find docs with 'prod' in tags");

    for i in [0, 50, 100, 250, 499] {
        let doc = mgr
            .get("test-metrics", &format!("metric-{}", i))
            .await
            .unwrap();
        assert!(doc.is_some(), "metric-{} should exist", i);
        let doc = doc.unwrap();
        assert!(
            doc.fields.contains_key("metric_name"),
            "missing metric_name on metric-{}",
            i
        );
        assert!(
            doc.fields.contains_key("host"),
            "missing host on metric-{}",
            i
        );
        assert!(
            doc.fields.contains_key("tags"),
            "missing tags on metric-{}",
            i
        );
    }
}

// ============================================================
// DOCUMENT UPDATE (re-index with same ID)
// ============================================================

#[tokio::test]
async fn test_logs_document_update() {
    let (_temp, mgr) = setup_logs_only().await;

    let doc_v1 = Document {
        id: "update-me".to_string(),
        fields: HashMap::from([
            ("line".to_string(), json!("Original kangaroo message alpha")),
            ("level".to_string(), json!("info")),
            ("source".to_string(), json!("v1-source")),
            ("app_id".to_string(), json!("app-001")),
            ("machine_id".to_string(), json!("m-east-1")),
            ("org_id".to_string(), json!("org-alpha")),
            ("node_id".to_string(), json!("node-a")),
        ]),
    };
    mgr.index("test-logs", vec![doc_v1]).await.unwrap();

    let v1 = mgr.get("test-logs", "update-me").await.unwrap().unwrap();
    assert_eq!(
        v1.fields.get("line").and_then(|v| v.as_str()),
        Some("Original kangaroo message alpha")
    );

    let doc_v2 = Document {
        id: "update-me".to_string(),
        fields: HashMap::from([
            ("line".to_string(), json!("Replacement zebra message beta")),
            ("level".to_string(), json!("error")),
            ("source".to_string(), json!("v2-source")),
            ("app_id".to_string(), json!("app-001")),
            ("machine_id".to_string(), json!("m-east-1")),
            ("org_id".to_string(), json!("org-alpha")),
            ("node_id".to_string(), json!("node-a")),
        ]),
    };
    mgr.index("test-logs", vec![doc_v2]).await.unwrap();

    let v2 = mgr.get("test-logs", "update-me").await.unwrap().unwrap();
    assert_eq!(
        v2.fields.get("line").and_then(|v| v.as_str()),
        Some("Replacement zebra message beta")
    );
    assert_eq!(
        v2.fields.get("level").and_then(|v| v.as_str()),
        Some("error")
    );
    assert_eq!(
        v2.fields.get("source").and_then(|v| v.as_str()),
        Some("v2-source")
    );

    let stats = mgr.stats("test-logs").await.unwrap();
    assert_eq!(
        stats.document_count, 1,
        "re-index should replace, not duplicate"
    );

    // "kangaroo" only existed in v1, should NOT be searchable after replacement
    let results = mgr
        .search(
            "test-logs",
            make_query_with_fields("kangaroo", vec!["line"], 10),
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.total, 0, "old content should not be searchable");

    // "zebra" only exists in v2, should be found
    let results = mgr
        .search(
            "test-logs",
            make_query_with_fields("zebra", vec!["line"], 10),
            None,
        )
        .await
        .unwrap();
    assert_eq!(results.total, 1, "new content should be searchable");
}

// ============================================================
// COLLECTION LIFECYCLE via add_collection (API-created)
// ============================================================

#[tokio::test]
async fn test_collection_created_at_runtime() {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager = Arc::new(
        CollectionManager::new(
            &schemas_dir,
            text_backend.clone(),
            vector_backend.clone(),
            None,
        )
        .unwrap(),
    );
    manager.initialize().await.unwrap();

    let schema: CollectionSchema = serde_yaml::from_str(log_schema_yaml()).unwrap();
    manager.add_collection(schema).await.unwrap();

    let docs = generate_log_docs(100, "runtime");
    manager.index("test-logs", docs).await.unwrap();

    let results = manager
        .search("test-logs", make_query("connection", 50), None)
        .await
        .unwrap();
    assert!(results.total > 0);

    let doc = manager.get("test-logs", "runtime-0").await.unwrap();
    assert!(doc.is_some());

    let schema_path = schemas_dir.join("test-logs.yaml");
    assert!(
        schema_path.exists(),
        "schema file should be written to disk"
    );

    // Simulate restart
    let text_backend2 = Arc::new(TextBackend::new(&data_dir).unwrap());
    let vector_backend2 = Arc::new(VectorBackend::new(&data_dir).unwrap());
    let manager2 = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend2, vector_backend2, None).unwrap(),
    );
    manager2.initialize().await.unwrap();

    assert!(
        manager2.collection_exists("test-logs"),
        "collection should survive restart"
    );

    let results2 = manager2
        .search("test-logs", make_query("connection", 50), None)
        .await
        .unwrap();
    assert!(results2.total > 0, "documents should survive restart");

    let doc2 = manager2.get("test-logs", "runtime-0").await.unwrap();
    assert!(doc2.is_some(), "get-by-ID should work after restart");
}

// ============================================================
// STRESS TEST — large volume
// ============================================================

#[tokio::test]
async fn test_logs_stress_1000_docs() {
    let (_temp, mgr) = setup_logs_only().await;
    let docs = generate_log_docs(1000, "stress");
    mgr.index("test-logs", docs).await.unwrap();

    let stats = mgr.stats("test-logs").await.unwrap();
    assert_eq!(stats.document_count, 1000);

    for i in [0, 100, 500, 999] {
        let doc = mgr
            .get("test-logs", &format!("stress-{}", i))
            .await
            .unwrap();
        assert!(doc.is_some(), "stress-{} should be retrievable", i);
    }

    let results = mgr
        .search(
            "test-logs",
            make_query("retrying OR timeout OR deployed OR webhook", 1000),
            None,
        )
        .await
        .unwrap();
    assert!(
        results.total > 100,
        "broad search should return many results from 1000 docs, got {}",
        results.total
    );
}

// ============================================================
// DELETE + RE-INDEX CYCLE
// ============================================================

#[tokio::test]
async fn test_logs_delete_reindex_cycle() {
    let (_temp, mgr) = setup_logs_only().await;

    for cycle in 0..5 {
        let docs = generate_log_docs(50, &format!("cycle{}", cycle));
        mgr.index("test-logs", docs).await.unwrap();

        if cycle > 0 {
            let ids: Vec<String> = (0..50)
                .map(|i| format!("cycle{}-{}", cycle - 1, i))
                .collect();
            mgr.delete("test-logs", ids).await.unwrap();
        }

        let doc = mgr
            .get("test-logs", &format!("cycle{}-0", cycle))
            .await
            .unwrap();
        assert!(
            doc.is_some(),
            "current batch doc should exist in cycle {}",
            cycle
        );

        let results = mgr
            .search("test-logs", make_query("connection OR timeout", 100), None)
            .await
            .unwrap();
        assert!(results.total > 0, "search should work in cycle {}", cycle);
    }
}

// ============================================================
// MULTI-COLLECTION CONCURRENT
// ============================================================

#[tokio::test]
async fn test_multi_collection_concurrent_operations() {
    let (_temp, mgr) = setup_environment().await;

    let mgr1 = mgr.clone();
    let mgr2 = mgr.clone();
    let mgr3 = mgr.clone();
    let mgr4 = mgr.clone();

    let t1 = tokio::spawn(async move {
        for i in 0..10 {
            let docs = generate_log_docs(20, &format!("conc-log-{}", i));
            mgr1.index("test-logs", docs).await.unwrap();
        }
    });

    let t2 = tokio::spawn(async move {
        for i in 0..10 {
            let start = i * 20;
            let docs: Vec<Document> = generate_metric_docs(200)
                .into_iter()
                .skip(start)
                .take(20)
                .enumerate()
                .map(|(j, mut d)| {
                    d.id = format!("conc-metric-{}-{}", i, j);
                    d
                })
                .collect();
            mgr2.index("test-metrics", docs).await.unwrap();
        }
    });

    let t3 = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        for _ in 0..20 {
            let _ = mgr3
                .search("test-logs", make_query("connection", 10), None)
                .await;
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    });

    let t4 = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        for _ in 0..20 {
            let _ = mgr4
                .search("test-metrics", make_query("cpu", 10), None)
                .await;
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    });

    let (r1, r2, r3, r4) = tokio::join!(t1, t2, t3, t4);
    r1.unwrap();
    r2.unwrap();
    r3.unwrap();
    r4.unwrap();
}

// ============================================================
// SEARCH BY STRING FIELDS (exact match)
// ============================================================

#[tokio::test]
async fn test_logs_search_string_fields() {
    let (_temp, mgr) = setup_logs_only().await;
    let docs = generate_log_docs(200, "sf");
    mgr.index("test-logs", docs).await.unwrap();

    let results = mgr
        .search(
            "test-logs",
            make_query_with_fields("api-gateway", vec!["source"], 200),
            None,
        )
        .await
        .unwrap();
    assert!(
        results.total > 0,
        "should find docs with source='api-gateway', got {}",
        results.total
    );

    let results = mgr
        .search(
            "test-logs",
            make_query_with_fields("error", vec!["level"], 200),
            None,
        )
        .await
        .unwrap();
    assert!(results.total > 0, "should find docs with level='error'");
}

// ============================================================
// EMPTY COLLECTION OPERATIONS
// ============================================================

#[tokio::test]
async fn test_logs_empty_collection() {
    let (_temp, mgr) = setup_logs_only().await;

    let results = mgr
        .search("test-logs", make_query("anything", 10), None)
        .await
        .unwrap();
    assert_eq!(results.total, 0);

    let stats = mgr.stats("test-logs").await.unwrap();
    assert_eq!(stats.document_count, 0);

    let doc = mgr.get("test-logs", "nonexistent").await.unwrap();
    assert!(doc.is_none());
}

// ============================================================
// COLLECTION NOT FOUND
// ============================================================

#[tokio::test]
async fn test_nonexistent_collection_errors() {
    let (_temp, mgr) = setup_logs_only().await;

    let result = mgr.index("no-such-collection", vec![]).await;
    assert!(result.is_err());

    let result = mgr
        .search("no-such-collection", make_query("test", 10), None)
        .await;
    assert!(result.is_err());
}
