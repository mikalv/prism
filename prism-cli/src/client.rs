//! Typed HTTP client for the Prism API.
use anyhow::{Context, Result};
use reqwest::Method;
use serde_json::Value;
use std::time::Duration;

pub const EXIT_CLIENT: i32 = 1;
pub const EXIT_SERVER_4XX: i32 = 2;
pub const EXIT_SERVER_5XX: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiErrorKind { Network, Server4xx, Server5xx }

#[derive(Debug)]
pub struct ApiError {
    pub kind: ApiErrorKind,
    pub status: Option<u16>,
    pub message: String,
}

impl ApiError {
    pub fn exit_code(&self) -> i32 {
        match self.kind {
            ApiErrorKind::Network => EXIT_CLIENT,
            ApiErrorKind::Server4xx => EXIT_SERVER_4XX,
            ApiErrorKind::Server5xx => EXIT_SERVER_5XX,
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.status, &self.kind) {
            (Some(s), _) => write!(f, "HTTP {}: {}", s, self.message),
            (None, ApiErrorKind::Network) => write!(f, "{}", self.message),
            (None, _) => write!(f, "{}", self.message),
        }
    }
}

pub struct PrismClient {
    http: reqwest::Client,
    base: String,
    api_key: Option<String>,
}

impl PrismClient {
    pub fn new(url: &str, api_key: Option<&str>, timeout: Duration, insecure: bool) -> Result<Self> {
        let mut builder = reqwest::Client::builder().timeout(timeout);
        if insecure {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let http = builder.build().context("Failed to build HTTP client")?;
        let base = url.trim_end_matches('/').to_string();
        if base.is_empty() || !base.starts_with("http") {
            anyhow::bail!("Invalid --url '{}'. Example: http://localhost:3080", url);
        }
        Ok(Self { http, base, api_key: api_key.map(str::to_string) })
    }

    /// Core request with retry: 3 attempts total on network errors and 5xx,
    /// backoff 500ms/1500ms. 4xx never retries. Returns parsed JSON body.
    pub async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, ApiError> {
        let url = format!("{}{}", self.base, path);
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let mut req = self.http.request(method.clone(), &url);
            if let Some(k) = &self.api_key {
                req = req.bearer_auth(k);
            }
            let req = match &body {
                Some(v) => req.json(v),
                None => req,
            };
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    if status.is_success() {
                        let parsed: Value = serde_json::from_str(&text).map_err(|e| ApiError {
                            kind: ApiErrorKind::Network,
                            status: Some(status.as_u16()),
                            message: format!("Invalid JSON from server: {}", e),
                        })?;
                        return Ok(parsed);
                    }
                    if status.is_client_error() {
                        return Err(ApiError {
                            kind: ApiErrorKind::Server4xx,
                            status: Some(status.as_u16()),
                            message: text,
                        });
                    }
                    if attempt < 3 {
                        tokio::time::sleep(Duration::from_millis(if attempt == 1 { 500 } else { 1500 })).await;
                        continue;
                    }
                    return Err(ApiError {
                        kind: ApiErrorKind::Server5xx,
                        status: Some(status.as_u16()),
                        message: text,
                    });
                }
                Err(e) => {
                    if attempt < 3 {
                        tokio::time::sleep(Duration::from_millis(if attempt == 1 { 500 } else { 1500 })).await;
                        continue;
                    }
                    return Err(ApiError {
                        kind: ApiErrorKind::Network,
                        status: None,
                        message: format!(
                            "Could not connect to {} — is the server running? Set --url or PRISM_URL. ({})",
                            self.base, e
                        ),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{get, post};
    use axum::Router;

    async fn spawn_stub() -> String {
        let app = Router::new()
            .route("/health", get(|| async { "{}" }))
            .route("/flaky", post(|| async {
                use std::sync::atomic::{AtomicU32, Ordering};
                static CALLS: AtomicU32 = AtomicU32::new(0);
                let n = CALLS.fetch_add(1, Ordering::SeqCst);
                if n < 2 { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") }
                else { (axum::http::StatusCode::OK, "{\"ok\": true}") }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn retries_5xx_then_succeeds() {
        let base = spawn_stub().await;
        let c = PrismClient::new(&base, None, Duration::from_secs(5), false).unwrap();
        let v = c.request(reqwest::Method::POST, "/flaky", None).await.unwrap();
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn network_error_is_reported_with_hint() {
        // port 1 on localhost: connection refused, immediate
        let c = PrismClient::new("http://127.0.0.1:1", None, Duration::from_secs(2), false).unwrap();
        let e = c.request(reqwest::Method::GET, "/health", None).await.unwrap_err();
        assert!(matches!(e.kind, ApiErrorKind::Network));
        assert!(e.message.contains("is the server running"));
    }
}
