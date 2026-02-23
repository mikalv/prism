use axum::{extract::Request, middleware::Next, response::Response};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

use super::types::AuthUser;
use crate::backends::Document;
use crate::collection::CollectionManager;

/// Routes excluded from audit logging
const AUDIT_SKIP: &[&str] = &["/health"];

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub timestamp: String,
    pub event_type: String,
    pub user: Option<String>,
    pub roles: Vec<String>,
    pub collection: Option<String>,
    pub action: String,
    pub status_code: u16,
    pub client_ip: String,
    pub duration_ms: u64,
}

impl AuditEvent {
    pub fn to_document(&self) -> Document {
        let mut fields = std::collections::HashMap::new();
        fields.insert(
            "timestamp".to_string(),
            serde_json::Value::String(self.timestamp.clone()),
        );
        fields.insert(
            "event_type".to_string(),
            serde_json::Value::String(self.event_type.clone()),
        );
        fields.insert(
            "user".to_string(),
            serde_json::Value::String(self.user.clone().unwrap_or_default()),
        );
        fields.insert(
            "roles".to_string(),
            serde_json::Value::String(self.roles.join(",")),
        );
        fields.insert(
            "collection".to_string(),
            serde_json::Value::String(self.collection.clone().unwrap_or_default()),
        );
        fields.insert(
            "action".to_string(),
            serde_json::Value::String(self.action.clone()),
        );
        fields.insert(
            "status_code".to_string(),
            serde_json::json!(self.status_code),
        );
        fields.insert(
            "client_ip".to_string(),
            serde_json::Value::String(self.client_ip.clone()),
        );
        fields.insert(
            "duration_ms".to_string(),
            serde_json::json!(self.duration_ms),
        );

        Document {
            id: uuid::Uuid::new_v4().to_string(),
            fields,
        }
    }
}

fn extract_collection(path: &str) -> Option<String> {
    let path = path.strip_prefix("/collections/")?;
    path.split('/').next().map(String::from)
}

fn classify_event(method: &axum::http::Method, path: &str) -> String {
    if path.starts_with("/admin/") {
        return "admin".to_string();
    }
    if path.contains("/search") || path.contains("/_suggest") || path.contains("/_mlt") {
        return "search".to_string();
    }
    if path.contains("/aggregate") {
        return "aggregate".to_string();
    }
    match *method {
        axum::http::Method::POST => "index".to_string(),
        axum::http::Method::DELETE => "delete".to_string(),
        _ => "read".to_string(),
    }
}

fn extract_client_ip(request: &Request) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub async fn audit_middleware(
    manager: Arc<CollectionManager>,
    index_to_collection: bool,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();

    // Skip audit for excluded routes
    if AUDIT_SKIP.iter().any(|s| path.starts_with(s)) {
        return next.run(request).await;
    }

    let method = request.method().clone();
    let client_ip = extract_client_ip(&request);
    let user = request.extensions().get::<AuthUser>().cloned();
    let collection = extract_collection(&path);
    let event_type = classify_event(&method, &path);
    let action = format!("{} {}", method, path);

    let start = Instant::now();
    let response = next.run(request).await;
    let duration_ms = start.elapsed().as_millis() as u64;

    let audit_event = AuditEvent {
        timestamp: Utc::now().to_rfc3339(),
        event_type,
        user: user.as_ref().map(|u| u.name.clone()),
        roles: user.as_ref().map(|u| u.roles.clone()).unwrap_or_default(),
        collection,
        action,
        status_code: response.status().as_u16(),
        client_ip,
        duration_ms,
    };

    // Log to tracing
    tracing::info!(
        target: "prism::audit",
        event_type = %audit_event.event_type,
        user = ?audit_event.user,
        collection = ?audit_event.collection,
        action = %audit_event.action,
        status_code = audit_event.status_code,
        client_ip = %audit_event.client_ip,
        duration_ms = audit_event.duration_ms,
        "audit"
    );

    // Index to _audit collection (fire-and-forget)
    if index_to_collection {
        let doc = audit_event.to_document();
        let mgr = manager.clone();
        tokio::spawn(async move {
            if let Err(e) = mgr.index("_audit", vec![doc]).await {
                tracing::warn!("Failed to index audit event: {}", e);
            }
        });
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- classify_event tests ---

    #[test]
    fn test_classify_admin_route() {
        assert_eq!(
            classify_event(&axum::http::Method::GET, "/admin/settings"),
            "admin"
        );
        assert_eq!(
            classify_event(&axum::http::Method::POST, "/admin/reload"),
            "admin"
        );
    }

    #[test]
    fn test_classify_search() {
        assert_eq!(
            classify_event(&axum::http::Method::POST, "/collections/test/search"),
            "search"
        );
        assert_eq!(
            classify_event(&axum::http::Method::GET, "/collections/test/_suggest"),
            "search"
        );
        assert_eq!(
            classify_event(&axum::http::Method::POST, "/collections/test/_mlt"),
            "search"
        );
    }

    #[test]
    fn test_classify_aggregate() {
        assert_eq!(
            classify_event(
                &axum::http::Method::POST,
                "/collections/test/aggregate"
            ),
            "aggregate"
        );
    }

    #[test]
    fn test_classify_index() {
        assert_eq!(
            classify_event(
                &axum::http::Method::POST,
                "/collections/test/documents"
            ),
            "index"
        );
    }

    #[test]
    fn test_classify_delete() {
        assert_eq!(
            classify_event(
                &axum::http::Method::DELETE,
                "/collections/test/documents/1"
            ),
            "delete"
        );
    }

    #[test]
    fn test_classify_read() {
        assert_eq!(
            classify_event(
                &axum::http::Method::GET,
                "/collections/test/documents/1"
            ),
            "read"
        );
        assert_eq!(
            classify_event(&axum::http::Method::PUT, "/collections/test"),
            "read"
        );
    }

    // --- extract_collection tests ---

    #[test]
    fn test_extract_collection_valid() {
        assert_eq!(
            extract_collection("/collections/my_index/documents"),
            Some("my_index".to_string())
        );
        assert_eq!(
            extract_collection("/collections/test/search"),
            Some("test".to_string())
        );
    }

    #[test]
    fn test_extract_collection_root() {
        assert_eq!(
            extract_collection("/collections/products"),
            Some("products".to_string())
        );
    }

    #[test]
    fn test_extract_collection_none() {
        assert_eq!(extract_collection("/health"), None);
        assert_eq!(extract_collection("/admin/settings"), None);
    }

    // --- extract_client_ip tests ---

    #[test]
    fn test_extract_client_ip_from_xff() {
        let mut req = Request::builder()
            .header("x-forwarded-for", "203.0.113.50, 70.41.3.18")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "203.0.113.50");
    }

    #[test]
    fn test_extract_client_ip_single() {
        let req = Request::builder()
            .header("x-forwarded-for", "192.168.1.1")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "192.168.1.1");
    }

    #[test]
    fn test_extract_client_ip_missing() {
        let req = Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "unknown");
    }

    // --- AuditEvent::to_document tests ---

    #[test]
    fn test_audit_event_to_document() {
        let event = AuditEvent {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            event_type: "search".to_string(),
            user: Some("admin".to_string()),
            roles: vec!["admin".to_string(), "viewer".to_string()],
            collection: Some("products".to_string()),
            action: "GET /collections/products/search".to_string(),
            status_code: 200,
            client_ip: "10.0.0.1".to_string(),
            duration_ms: 42,
        };

        let doc = event.to_document();
        assert!(!doc.id.is_empty());
        assert_eq!(
            doc.fields.get("event_type"),
            Some(&serde_json::Value::String("search".to_string()))
        );
        assert_eq!(
            doc.fields.get("user"),
            Some(&serde_json::Value::String("admin".to_string()))
        );
        assert_eq!(
            doc.fields.get("roles"),
            Some(&serde_json::Value::String("admin,viewer".to_string()))
        );
        assert_eq!(
            doc.fields.get("collection"),
            Some(&serde_json::Value::String("products".to_string()))
        );
        assert_eq!(doc.fields.get("status_code"), Some(&serde_json::json!(200)));
        assert_eq!(doc.fields.get("duration_ms"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn test_audit_event_to_document_none_fields() {
        let event = AuditEvent {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            event_type: "read".to_string(),
            user: None,
            roles: vec![],
            collection: None,
            action: "GET /health".to_string(),
            status_code: 200,
            client_ip: "unknown".to_string(),
            duration_ms: 1,
        };

        let doc = event.to_document();
        assert_eq!(
            doc.fields.get("user"),
            Some(&serde_json::Value::String("".to_string()))
        );
        assert_eq!(
            doc.fields.get("collection"),
            Some(&serde_json::Value::String("".to_string()))
        );
        assert_eq!(
            doc.fields.get("roles"),
            Some(&serde_json::Value::String("".to_string()))
        );
    }
}
