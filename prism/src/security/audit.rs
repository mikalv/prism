use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

use super::types::AuthUser;
use crate::collection::CollectionManager;
use crate::backends::Document;

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
        fields.insert("timestamp".to_string(), serde_json::Value::String(self.timestamp.clone()));
        fields.insert("event_type".to_string(), serde_json::Value::String(self.event_type.clone()));
        fields.insert("user".to_string(), serde_json::Value::String(self.user.clone().unwrap_or_default()));
        fields.insert("roles".to_string(), serde_json::Value::String(self.roles.join(",")));
        fields.insert("collection".to_string(), serde_json::Value::String(self.collection.clone().unwrap_or_default()));
        fields.insert("action".to_string(), serde_json::Value::String(self.action.clone()));
        fields.insert("status_code".to_string(), serde_json::json!(self.status_code));
        fields.insert("client_ip".to_string(), serde_json::Value::String(self.client_ip.clone()));
        fields.insert("duration_ms".to_string(), serde_json::json!(self.duration_ms));

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
