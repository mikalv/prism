use std::env;

#[tokio::test]
async fn full_lifecycle() {
    let url = match env::var("PRISM_TEST_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("Skipping integration test: PRISM_TEST_URL not set");
            return;
        }
    };

    let client = prismsearch::Client::new(&url).build().unwrap();
    let test_collection = format!("prismsearch_rs_test_{}", rand_suffix());

    // Health
    let health = client.health().await.unwrap();
    assert_eq!(health.status, "ok");

    // Create collection
    let schema = serde_json::json!({
        "backends": {
            "text": {
                "fields": [
                    {"name": "title", "type": "text", "stored": true, "indexed": true},
                    {"name": "content", "type": "text", "stored": true, "indexed": true}
                ]
            }
        }
    });
    client
        .create_collection(&test_collection, &schema)
        .await
        .unwrap();

    // Index documents
    let docs = vec![
        prismsearch::Document::new("1")
            .field("title", "Rust Testing")
            .field("content", "Integration test doc"),
        prismsearch::Document::new("2")
            .field("title", "Tokio Runtime")
            .field("content", "Async runtime for Rust"),
    ];
    let result = client.index(&test_collection, &docs).await.unwrap();
    assert_eq!(result.indexed, 2);

    // Wait for indexing
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Search
    let results = prismsearch::Query::new(&test_collection, "Rust")
        .execute(&client)
        .await
        .unwrap();
    assert!(results.total > 0);

    // List collections
    let collections = client.list_collections().await.unwrap();
    assert!(collections.contains(&test_collection));

    // Cleanup
    client.delete_collection(&test_collection).await.unwrap();
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("{:x}", t)
}
