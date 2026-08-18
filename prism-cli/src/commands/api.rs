//! Handlers for API-mode commands (prismctl's first-class client surface).
use crate::client::PrismClient;
use crate::output::{print_collections, CollectionRow, Output};
use anyhow::Result;

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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;

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
}
