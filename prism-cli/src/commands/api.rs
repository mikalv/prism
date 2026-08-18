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

/// `prismctl graph edges <collection> <node>` — list edges from a node.
pub async fn run_graph_edges(o: &ApiOpts, collection: &str, node: &str) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let v = match client.request(reqwest::Method::GET,
        &format!("/collections/{}/graph/nodes/{}/edges", collection, node), None).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_graph_edges(&out, &v);
    Ok(0)
}

/// `prismctl graph bfs <collection> <node>` — breadth-first traversal by edge type.
pub async fn run_graph_bfs(o: &ApiOpts, collection: &str, node: &str, edge_type: &str, depth: usize) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let body = serde_json::json!({ "start": node, "edge_type": edge_type, "max_depth": depth });
    let v = match client.request(reqwest::Method::POST,
        &format!("/collections/{}/graph/bfs", collection), Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_graph_bfs(&out, &v);
    Ok(0)
}

/// `prismctl graph path <collection> <from> <to>` — shortest path between nodes.
pub async fn run_graph_path(o: &ApiOpts, collection: &str, from: &str, to: &str) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let body = serde_json::json!({ "start": from, "target": to });
    let v = match client.request(reqwest::Method::POST,
        &format!("/collections/{}/graph/shortest-path", collection), Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_graph_path(&out, &v);
    Ok(0)
}

/// `prismctl graph stats <collection>` — node/edge counts for the graph backend.
pub async fn run_graph_stats(o: &ApiOpts, collection: &str) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let v = match client.request(reqwest::Method::GET,
        &format!("/collections/{}/graph/stats", collection), None).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_graph_stats(&out, &v);
    Ok(0)
}

/// `prismctl suggest <collection> <prefix> --field <f>` — autocomplete suggestions.
pub async fn run_suggest(o: &ApiOpts, collection: &str, prefix: &str, field: &str, size: usize) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let body = serde_json::json!({ "prefix": prefix, "field": field, "size": size, "fuzzy": false, "max_distance": 2 });
    let v = match client.request(reqwest::Method::POST,
        &format!("/collections/{}/_suggest", collection), Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_suggest(&out, &v);
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

/// `prismctl backup-key` — ask the server to generate an AES-256 key.
/// The key is printed once; it is required for restore, so it must be stored
/// in a secrets manager. Never logged by the server or this client.
pub async fn run_backup_keygen(o: &ApiOpts) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let v = match client.request(reqwest::Method::POST, "/_admin/encryption/generate-key", None).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    if out.is_json() {
        out.raw(&v);
        return Ok(0);
    }
    let key = v.get("key").and_then(|x| x.as_str()).unwrap_or("?");
    println!("key: {}", key);
    println!("algorithm: {}", v.get("algorithm").and_then(|x| x.as_str()).unwrap_or("?"));
    eprintln!("WARNING: store this key in a secrets manager — it is needed for restore and cannot be regenerated.");
    Ok(0)
}

/// `prismctl backup <collection> <output_path> [--key HEX]` — encrypted backup.
/// NOTE: output_path is a path ON THE SERVER, not local. If --key is omitted,
/// a key is generated via keygen first and printed to stderr with instructions.
pub async fn run_backup(o: &ApiOpts, collection: &str, output_path: &str, key: Option<&str>) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    let key = match key {
        Some(k) => k.to_string(),
        None => {
            let v = match client.request(reqwest::Method::POST, "/_admin/encryption/generate-key", None).await {
                Ok(v) => v,
                Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
            };
            let k = v.get("key").and_then(|x| x.as_str())
                .ok_or_else(|| anyhow::anyhow!("server did not return a key"))?.to_string();
            eprintln!("note: no --key given; generated a new encryption key:");
            eprintln!("  key: {}", k);
            eprintln!("  store it in a secrets manager — it is needed for restore and cannot be regenerated.");
            k
        }
    };
    eprintln!("note: output_path is a path ON THE SERVER, not local");
    let body = serde_json::json!({
        "collection": collection,
        "output_path": output_path,
        "key": key,
    });
    let v = match client.request(reqwest::Method::POST, "/_admin/export/encrypted", Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_backup(&out, &v);
    Ok(0)
}

/// `prismctl restore <input_path> --key HEX [--target-collection NAME]` —
/// restore an encrypted backup. NOTE: input_path is a path ON THE SERVER.
pub async fn run_restore(o: &ApiOpts, input_path: &str, key: &str, target_collection: Option<String>) -> Result<i32> {
    let client = make_client(o)?;
    let out = make_output(o);
    eprintln!("note: input_path is a path ON THE SERVER");
    let mut body = serde_json::json!({
        "input_path": input_path,
        "key": key,
    });
    if let Some(t) = target_collection {
        body.as_object_mut().unwrap().insert("target_collection".into(), serde_json::json!(t));
    }
    let v = match client.request(reqwest::Method::POST, "/_admin/import/encrypted", Some(body)).await {
        Ok(v) => v,
        Err(e) => { eprintln!("error: {}", e); return Ok(e.exit_code()); }
    };
    crate::output::print_backup(&out, &v);
    Ok(0)
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

    #[tokio::test]
    async fn backup_auto_generates_key_when_missing() {
        let key_calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let kc = key_calls.clone();
        let app = Router::new()
            .route("/_admin/encryption/generate-key", post(move || {
                let kc = kc.clone();
                async move { kc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    axum::Json(serde_json::json!({"key": "a".repeat(64), "key_bytes": 32})) }
            }))
            .route("/_admin/export/encrypted", post(|body: String| async move {
                assert!(body.contains("\"key\""));
                axum::Json(serde_json::json!({"success": true, "collection": "c", "output_path": "/tmp/x", "size_bytes": 10}))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_backup(&opts, "c", "/tmp/x", None).await.unwrap();
        assert_eq!(code, 0);
        assert_eq!(key_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn backup_with_explicit_key_skips_keygen() {
        let key_calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let kc = key_calls.clone();
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new()
            .route("/_admin/encryption/generate-key", post(move || {
                let kc = kc.clone();
                async move { kc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    axum::Json(serde_json::json!({"key": "a".repeat(64), "key_bytes": 32})) }
            }))
            .route("/_admin/export/encrypted", post(move |body: String| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = body;
                    axum::Json(serde_json::json!({"success": true, "collection": "c", "output_path": "/tmp/x", "size_bytes": 10}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_backup(&opts, "c", "/tmp/x", Some("b".repeat(64).as_str())).await.unwrap();
        assert_eq!(code, 0);
        assert_eq!(key_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(captured.lock().unwrap().contains(&"b".repeat(64)));
    }

    #[tokio::test]
    async fn backup_maps_server_error_to_exit_code() {
        let app = Router::new()
            .route("/_admin/export/encrypted", post(|| async {
                (axum::http::StatusCode::BAD_REQUEST,
                 axum::Json(serde_json::json!({"success": false, "collection": "c", "output_path": "/tmp/x", "size_bytes": 0, "error": "Invalid key"})))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_backup(&opts, "c", "/tmp/x", Some("k")).await.unwrap();
        assert_eq!(code, 2);
    }

    #[tokio::test]
    async fn backup_keygen_fetches_endpoint() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/_admin/encryption/generate-key", post(move || {
            let c2 = c2.clone();
            async move { *c2.lock().unwrap() = "hit".into();
                axum::Json(serde_json::json!({"key": "a".repeat(64), "key_bytes": 32, "algorithm": "AES-256-GCM"})) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_backup_keygen(&opts).await.unwrap();
        assert_eq!(code, 0);
        assert_eq!(captured.lock().unwrap().as_str(), "hit");
    }

    #[tokio::test]
    async fn restore_posts_body_and_exits_zero() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/_admin/import/encrypted",
            post(move |body: String| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = body;
                    axum::Json(serde_json::json!({"success": true, "collection": "c", "files_extracted": 4, "bytes_extracted": 512}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_restore(&opts, "/srv/backup/c.enc", "a".repeat(64).as_str(), Some("c2".to_string())).await.unwrap();
        assert_eq!(code, 0);
        let cap = captured.lock().unwrap().clone();
        assert!(cap.contains("/srv/backup/c.enc"));
        assert!(cap.contains(&"a".repeat(64)));
        assert!(cap.contains("\"target_collection\":\"c2\"") || cap.contains("\"target_collection\": \"c2\""));
    }

    #[tokio::test]
    async fn restore_omits_target_collection_when_none() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/_admin/import/encrypted",
            post(move |body: String| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = body;
                    axum::Json(serde_json::json!({"success": true, "collection": "c", "files_extracted": 1, "bytes_extracted": 8}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_restore(&opts, "/srv/backup/c.enc", "a".repeat(64).as_str(), None).await.unwrap();
        assert_eq!(code, 0);
        assert!(!captured.lock().unwrap().contains("target_collection"));
    }

    #[tokio::test]
    async fn restore_maps_server_error_to_exit_code() {
        let app = Router::new().route("/_admin/import/encrypted", post(|| async {
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
             axum::Json(serde_json::json!({"success": false, "collection": "", "files_extracted": 0, "bytes_extracted": 0, "error": "decryption failed"})))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_restore(&opts, "/x.enc", "k", None).await.unwrap();
        assert_eq!(code, 3);
    }

    #[tokio::test]
    async fn graph_bfs_posts_start_and_depth() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/collections/:c/graph/bfs",
            post(move |body: String| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = body;
                    axum::Json(serde_json::json!({"nodes": ["a","b"], "count": 2}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_graph_bfs(&opts, "g", "root", "relates", 4).await.unwrap();
        assert_eq!(code, 0);
        let cap = captured.lock().unwrap().clone();
        assert!(cap.contains("\"start\":\"root\""));
        assert!(cap.contains("\"max_depth\":4"));
    }

    #[tokio::test]
    async fn suggest_posts_prefix_and_field() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let c2 = captured.clone();
        let app = Router::new().route("/collections/:c/_suggest",
            post(move |body: String| {
                let c2 = c2.clone();
                async move {
                    *c2.lock().unwrap() = body;
                    axum::Json(serde_json::json!({"suggestions": ["prism","privacy"]}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let opts = ApiOpts { url: Some(format!("http://{}", addr)), api_key: None, timeout: 5, insecure: false, json: false };
        let code = run_suggest(&opts, "s", "pri", "title", 5).await.unwrap();
        assert_eq!(code, 0);
        let cap = captured.lock().unwrap().clone();
        assert!(cap.contains("\"prefix\":\"pri\"") && cap.contains("\"field\":\"title\""));
    }
}
