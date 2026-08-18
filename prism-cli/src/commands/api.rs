//! Handlers for API-mode commands (prismctl's first-class client surface).
use crate::client::PrismClient;
use crate::output::{print_collections, print_document, CollectionRow, Output};
use anyhow::{Context, Result};
use std::io::BufRead as _;

#[derive(Debug, Clone)]
pub struct ApiOpts {
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub timeout: u64,
    pub insecure: bool,
    pub json: bool,
}

pub fn make_client(o: &ApiOpts) -> Result<PrismClient> {
    let url = o.url.clone().ok_or_else(|| anyhow::anyhow!(
        "No server URL. Pass --url or set PRISM_URL (e.g. http://localhost:3080)"))?;
    PrismClient::new(&url, o.api_key.as_deref(), std::time::Duration::from_secs(o.timeout), o.insecure)
}

pub fn make_output(o: &ApiOpts) -> Output { Output::new(o.json) }

/// `prismctl collections` — list collections with doc counts and sizes.
/// GET /admin/collections, then fan out GET /collections/:c/stats for each
/// (failures tolerated: row shows docs=0 bytes=0, since stats is best-effort).
pub async fn run_collections(o: &ApiOpts) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let v = match client.request(reqwest::Method::GET, "/admin/collections", None).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    let mut names: Vec<String> = v.get("collections")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
        .unwrap_or_default();
    names.sort();
    let mut rows = Vec::with_capacity(names.len());
    for name in &names {
        let stats = client.request(reqwest::Method::GET,
            &format!("/collections/{}/stats", name), None).await.ok();
        let (docs, bytes) = match stats {
            Some(s) => (
                s.get("document_count").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                s.get("storage_bytes").and_then(|x| x.as_u64()).unwrap_or(0)),
            None => (0, 0),
        };
        rows.push(CollectionRow { name: name.clone(), docs, bytes });
    }
    print_collections(&out, &rows);
    Ok(0)
}

/// `prismctl search <collection> <query>` — mode is merge_strategy: hybrid (rrf default),
/// vector, or text. vector/text set the server-side weights so one engine dominates.
pub async fn run_search(o: &ApiOpts, collection: &str, query: &str, mode: &str, limit: usize, weights: Option<(f32, f32)>) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let mut body = serde_json::json!({
        "query": query,
        "limit": limit,
    });
    let obj = body.as_object_mut().unwrap();
    match mode {
        "hybrid" => { obj.insert("merge_strategy".into(), "rrf".into()); }
        "vector" => { obj.insert("merge_strategy".into(), "weighted".into());
                      obj.insert("text_weight".into(), serde_json::json!(0.0));
                      obj.insert("vector_weight".into(), serde_json::json!(1.0)); }
        "text" => { obj.insert("merge_strategy".into(), "weighted".into());
                    obj.insert("text_weight".into(), serde_json::json!(1.0));
                    obj.insert("vector_weight".into(), serde_json::json!(0.0)); }
        other => { anyhow::bail!("unknown mode '{}'; expected hybrid|vector|text", other); }
    }
    if let Some((tw, vw)) = weights {
        obj.insert("text_weight".into(), serde_json::json!(tw));
        obj.insert("vector_weight".into(), serde_json::json!(vw));
    }
    let v = match client.request(reqwest::Method::POST,
        &format!("/collections/{}/search", collection), Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_search(&out, &v);
    Ok(0)
}

/// Open a JSONL source: `-` = stdin, otherwise a file path.
fn read_source(file: &str) -> Result<Box<dyn std::io::BufRead>> {
    if file == "-" {
        Ok(Box::new(std::io::BufReader::new(std::io::stdin())))
    } else {
        Ok(Box::new(std::io::BufReader::new(std::fs::File::open(file)?)))
    }
}

/// Print per-document errors from an index response, if any.
fn print_doc_errors(v: &serde_json::Value) {
    if let Some(errs) = v.get("errors").and_then(serde_json::Value::as_array) {
        for e in errs {
            eprintln!("error: {}: {}",
                e.get("doc_id").and_then(serde_json::Value::as_str).unwrap_or("?"),
                e.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown"));
        }
    }
}

/// `prismctl reindex <patterns...> [--batch-size N]` — strip stored embeddings
/// and regenerate them with the active provider. Server validates the same
/// rules; the client pre-validates batch_size so bogus values fail before any I/O.
pub async fn run_reindex(o: &ApiOpts, collections: Vec<String>, batch_size: usize) -> Result<i32> {
    if batch_size == 0 || batch_size > 1000 {
        anyhow::bail!("`batch_size` must be between 1 and 1000");
    }
    let client = make_client(o)?;
    let out = make_output(o);
    let body = serde_json::json!({ "collections": collections, "batch_size": batch_size });
    let v = match client.request(reqwest::Method::POST, "/admin/reindex", Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_reindex(&out, &v);
    Ok(0)
}

/// `prismctl schema get <collection>` — print a collection's schema.
pub async fn run_schema_get(o: &ApiOpts, collection: &str) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let v = match client.request(reqwest::Method::GET,
        &format!("/collections/{}/schema", collection), None).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_schema(&out, collection, &v);
    Ok(0)
}

/// `prismctl schema lint` — report schema issues across all collections.
pub async fn run_schema_lint(o: &ApiOpts) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let v = match client.request(reqwest::Method::GET, "/admin/lint-schemas", None).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_lint(&out, &v);
    Ok(0)
}

/// `prismctl doc get <collection> <id>` — fetch one document.
/// A `null` body is a result (not found), not an error: exit 0 either way.
pub async fn run_doc_get(o: &ApiOpts, collection: &str, id: &str) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let v = match client.request(reqwest::Method::GET,
        &format!("/collections/{}/documents/{}", collection, id), None).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    print_document(&out, &v);
    Ok(0)
}

/// `prismctl doc index <collection> <file>` — index one JSON document.
/// `<file>` is a path, `-` for stdin, or an inline JSON object.
/// Wraps into `{"documents":[{...}]}` and posts with `?sync=true`.
pub async fn run_doc_index(o: &ApiOpts, collection: &str, file: &str) -> Result<i32> {
    let raw = if file == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
        s
    } else if std::path::Path::new(file).is_file() {
        std::fs::read_to_string(file)?
    } else {
        file.to_string()
    };
    if raw.trim().is_empty() {
        anyhow::bail!("empty input");
    }
    let doc: serde_json::Value = serde_json::from_str(&raw).context("input is not valid JSON")?;
    if !doc.is_object() {
        anyhow::bail!("input must be a JSON object (a single document)");
    }
    if doc.get("id").and_then(|v| v.as_str()).is_none() {
        anyhow::bail!("document must have an `id` field (string)");
    }
    let client = make_client(o)?;
    let out = make_output(o);
    let body = serde_json::json!({ "documents": [doc] });
    let v = match client.request(reqwest::Method::POST,
        &format!("/collections/{}/documents?sync=true", collection), Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    let indexed = v.get("indexed").and_then(|x| x.as_u64()).unwrap_or(0);
    let failed = v.get("failed").and_then(|x| x.as_u64()).unwrap_or(0);
    if out.is_json() {
        out.raw(&v);
    } else {
        println!("indexed={} failed={}", indexed, failed);
    }
    print_doc_errors(&v);
    Ok(if failed > 0 { 2 } else { 0 })
}

/// `prismctl doc delete <collection> <id>` — delete one document.
/// The server is idempotent: a missing doc returns 200 with `result: "not_found"`.
pub async fn run_doc_delete(o: &ApiOpts, collection: &str, id: &str) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let v = match client.request(reqwest::Method::DELETE,
        &format!("/collections/{}/documents/{}", collection, id), None).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    let result = v.get("result").and_then(|x| x.as_str()).unwrap_or("?");
    if out.is_json() {
        out.raw(&v);
    } else if result == "deleted" {
        println!("deleted {}", id);
    } else {
        println!("not found: {}", id);
    }
    Ok(if result == "deleted" { 0 } else { 2 })
}

/// `prismctl doc bulk <collection> <file> [--batch-size N]` — JSONL bulk import.
/// Batches documents (default 100) into `{"documents":[...]}` posts with `?sync=true`.
pub async fn run_doc_bulk(o: &ApiOpts, collection: &str, file: &str, batch_size: usize) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let reader = read_source(file)?;
    let batch_size = batch_size.max(1);
    let mut batch: Vec<serde_json::Value> = Vec::with_capacity(batch_size);
    let mut total_indexed: u64 = 0;
    let mut total_failed: u64 = 0;
    let mut total_seen: usize = 0;

    for line in reader.lines() {
        let line: String = line.context("failed to read input")?;
        if line.trim().is_empty() {
            continue;
        }
        let doc: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("input is not valid JSON: {}", &line[..line.len().min(100)]))?;
        batch.push(doc);
        if batch.len() >= batch_size {
            let (indexed, failed) = post_batch(&client, collection, &batch).await?;
            total_indexed += indexed;
            total_failed += failed;
            total_seen += batch.len();
            if total_seen % 1000 < batch.len() {
                eprint!("\r  {} docs ({} failed)", total_seen, total_failed);
            }
            batch.clear();
        }
    }
    if !batch.is_empty() {
        let (indexed, failed) = post_batch(&client, collection, &batch).await?;
        total_indexed += indexed;
        total_failed += failed;
    }
    eprintln!();
    if out.is_json() {
        out.raw(&serde_json::json!({
            "collection": collection,
            "indexed": total_indexed,
            "failed": total_failed,
        }));
    } else {
        println!("indexed={} failed={}", total_indexed, total_failed);
    }
    Ok(if total_failed > 0 { 2 } else { 0 })
}

/// POST one batch of documents with sync indexing; returns (indexed, failed).
/// Per-doc errors are printed as they arrive.
async fn post_batch(client: &PrismClient, collection: &str, batch: &[serde_json::Value])
    -> Result<(u64, u64)> {
    let body = serde_json::json!({ "documents": batch });
    let v = match client.request(reqwest::Method::POST,
        &format!("/collections/{}/documents?sync=true", collection), Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); anyhow::bail!("batch failed"); }
    };
    print_doc_errors(&v);
    let indexed = v.get("indexed").and_then(|x| x.as_u64()).unwrap_or(0);
    let failed = v.get("failed").and_then(|x| x.as_u64()).unwrap_or(0);
    Ok((indexed, failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{get, post};
    use axum::Router;

    #[tokio::test]
    async fn search_posts_body_and_exits_zero() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/collections/:c/search",
            post(move |axum::extract::Path(c): axum::extract::Path<String>, axum::extract::Json(body): axum::extract::Json<serde_json::Value>| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = format!("{}|{}", c, body);
                    axum::Json(serde_json::json!({"total": 0, "results": [], "latency_ms": 1}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_search(&opts, "docs", "privacy tools", "hybrid", 10, None).await.unwrap();
        assert_eq!(code, 0);
        let cap = captured.lock().unwrap().clone();
        assert!(cap.contains("docs|"));
        assert!(cap.contains("privacy tools"));
        assert!(cap.contains("\"merge_strategy\":\"rrf\"") || cap.contains("\"merge_strategy\": \"rrf\""));
    }

    #[test]
    fn search_rejects_unknown_mode() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let opts = ApiOpts { url: Some("http://127.0.0.1:1".into()), api_key: None, timeout: 1, insecure: false, json: false };
        let err = rt.block_on(run_search(&opts, "docs", "q", "bogus", 10, None)).unwrap_err();
        assert!(err.to_string().contains("unknown mode 'bogus'"));
    }

    #[tokio::test]
    async fn search_vector_mode_sets_weighted_weights() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/collections/:c/search",
            post(move |axum::extract::Json(body): axum::extract::Json<serde_json::Value>| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = body.to_string();
                    axum::Json(serde_json::json!({"total": 0, "results": [], "latency_ms": 1}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_search(&opts, "docs", "q", "vector", 5, None).await.unwrap();
        assert_eq!(code, 0);
        let cap = captured.lock().unwrap().clone();
        assert!(cap.contains("\"merge_strategy\":\"weighted\"") || cap.contains("\"merge_strategy\": \"weighted\""));
        assert!(cap.contains("\"text_weight\":0.0"));
        assert!(cap.contains("\"vector_weight\":1.0"));
    }

    #[tokio::test]
    async fn doc_index_wraps_single_document() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/collections/:c/documents",
            post(move |axum::extract::Path(c): axum::extract::Path<String>,
                  axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
                  axum::extract::Json(body): axum::extract::Json<serde_json::Value>| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = format!("{}|{}|{}", c, q.get("sync").map(String::as_str).unwrap_or("?"), body);
                    axum::Json(serde_json::json!({"indexed": 1, "failed": 0}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_doc_index(&opts, "docs", r#"{"id": "a1", "title": "t"}"#).await.unwrap();
        assert_eq!(code, 0);
        let cap = captured.lock().unwrap().clone();
        assert!(cap.contains("docs|true|"));
        assert!(cap.contains("\"documents\""));
    }

    #[test]
    fn doc_index_rejects_missing_id() {
        // no server needed: validation happens before the request
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let opts = ApiOpts { url: Some("http://127.0.0.1:1".into()), api_key: None, timeout: 1, insecure: false, json: false };
        let err = rt.block_on(run_doc_index(&opts, "docs", r#"{"title": "no id"}"#)).unwrap_err();
        assert!(err.to_string().contains("must have an `id`"));
    }

    #[tokio::test]
    async fn doc_get_null_is_not_found_exit_zero() {
        let app = Router::new().route("/collections/:c/documents/:id",
            get(|| async { axum::Json(serde_json::Value::Null) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_doc_get(&opts, "docs", "missing").await.unwrap();
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn collections_lists_with_stats() {
        let app = Router::new()
            .route("/admin/collections", get(|| async {
                axum::Json(serde_json::json!({"collections": ["alpha", "beta"]}))
            }))
            .route("/collections/:c/stats", get(|axum::extract::Path(c): axum::extract::Path<String>| async move {
                axum::Json(serde_json::json!({"collection": c, "document_count": 7, "storage_bytes": 1024}))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        // capture stdout by asserting exit code only; rendering covered by output tests
        let code = run_collections(&opts).await.unwrap();
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn reindex_posts_patterns_and_batch_size() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/admin/reindex",
            post(move |axum::extract::Json(body): axum::extract::Json<serde_json::Value>| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = body.to_string();
                    axum::Json(serde_json::json!({"collections":[{"collection":"x","reembedded":3,"skipped":0}],"total_reembedded":3,"total_skipped":0}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_reindex(&opts, vec!["idx_*".into(), "darknet_web".into()], 100).await.unwrap();
        assert_eq!(code, 0);
        assert!(captured.lock().unwrap().contains("idx_*"));
        assert!(captured.lock().unwrap().contains("darknet_web"));
        assert!(captured.lock().unwrap().contains("\"batch_size\":100"));
    }

    #[test]
    fn reindex_rejects_bad_batch_size_client_side() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let opts = ApiOpts { url: Some("http://127.0.0.1:1".into()), api_key: None, timeout: 1, insecure: false, json: false };
        assert!(rt.block_on(run_reindex(&opts, vec!["x".into()], 0)).is_err());
        assert!(rt.block_on(run_reindex(&opts, vec!["x".into()], 1001)).is_err());
    }

    #[tokio::test]
    async fn reindex_no_match_is_4xx_exit_code() {
        // server rejects patterns matching nothing with 400; client maps 4xx -> exit 2
        let app = Router::new().route("/admin/reindex",
            post(|| async { (axum::http::StatusCode::BAD_REQUEST, "no collections matched") }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_reindex(&opts, vec!["nonexistent*".into()], 100).await.unwrap();
        assert_eq!(code, 2);
    }

    #[tokio::test]
    async fn schema_get_fetches_endpoint() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/collections/:c/schema",
            get(move |axum::extract::Path(c): axum::extract::Path<String>| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = c.clone();
                    axum::Json(serde_json::json!({"collection": c, "fields": {}}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_schema_get(&opts, "idx_web").await.unwrap();
        assert_eq!(code, 0);
        assert_eq!(captured.lock().unwrap().as_str(), "idx_web");
    }

    #[tokio::test]
    async fn schema_lint_fetches_endpoint() {
        let app = Router::new().route("/admin/lint-schemas",
            get(|| async { axum::Json(serde_json::json!({"idx_web": ["field 'x' has no type"]})) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_schema_lint(&opts).await.unwrap();
        assert_eq!(code, 0);
    }
}
