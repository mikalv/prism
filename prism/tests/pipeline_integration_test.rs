//! Integration tests for ingest pipelines

use prism::api::ApiServer;
use prism::backends::text::TextBackend;
use prism::backends::VectorBackend;
use prism::collection::CollectionManager;
use prism::config::{CorsConfig, SecurityConfig};
use prism::pipeline::registry::PipelineRegistry;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};

async fn setup_server(pipelines_yaml: &[(&str, &str)]) -> (TempDir, String) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    let pipelines_dir = temp.path().join("pipelines");
    std::fs::create_dir_all(&schemas_dir).unwrap();
    std::fs::create_dir_all(&pipelines_dir).unwrap();

    for (name, content) in pipelines_yaml {
        std::fs::write(pipelines_dir.join(name), content).unwrap();
    }

    let text_backend = Arc::new(TextBackend::new(temp.path()).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(temp.path()).unwrap());
    let manager =
        Arc::new(CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap());

    let registry = PipelineRegistry::load(&pipelines_dir).unwrap();
    let server = ApiServer::with_pipelines(
        manager,
        CorsConfig::default(),
        SecurityConfig::default(),
        registry,
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, server.router().await).await.unwrap();
    });

    sleep(Duration::from_millis(50)).await;
    (temp, url)
}

#[tokio::test]
async fn test_list_pipelines_empty() {
    let (_temp, url) = setup_server(&[]).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/pipelines", url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["pipelines"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_list_pipelines_with_one() {
    let yaml = r#"
name: normalize
description: Test pipeline
processors:
  - lowercase:
      field: title
"#;
    let (_temp, url) = setup_server(&[("normalize.yaml", yaml)]).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/pipelines", url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let pipelines = body["pipelines"].as_array().unwrap();
    assert_eq!(pipelines.len(), 1);
    assert_eq!(pipelines[0]["name"], "normalize");
    assert_eq!(pipelines[0]["processor_count"], 1);
}

#[tokio::test]
async fn test_unknown_pipeline_returns_400() {
    let (_temp, url) = setup_server(&[]).await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{}/collections/test/documents?pipeline=nonexistent",
            url
        ))
        .json(&serde_json::json!({
            "documents": [{"id": "1", "fields": {"title": "hello"}}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}
