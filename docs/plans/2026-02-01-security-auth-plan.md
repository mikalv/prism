# Security: API Keys, RBAC, and Audit Logging — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add API key authentication, collection-level RBAC with glob patterns, and audit logging (to tracing + self-indexed `_audit` collection) to Prism.

**Architecture:** A new `security` module provides auth types, permission checking, and two axum middleware layers (auth + audit). When `security.enabled = false` (default), no middleware is added. Audit logging is independent — can run without auth. Audit events are indexed into an internal `_audit` collection for dogfooding.

**Tech Stack:** axum middleware (tower Layer), glob pattern matching, serde config, tracing structured logging

---

### Task 1: Add SecurityConfig structs

**Files:**
- Modify: `/home/meeh/prism/prism/src/config/mod.rs` (add SecurityConfig after line 29, update Config struct and Default impl)
- Test: `/home/meeh/prism/prism/tests/config_test.rs`

**Step 1: Write the failing tests**

Append to `/home/meeh/prism/prism/tests/config_test.rs`:

```rust
#[test]
fn test_default_security_config() {
    let config = Config::default();
    assert!(!config.security.enabled);
    assert!(config.security.api_keys.is_empty());
    assert!(config.security.roles.is_empty());
    assert!(!config.security.audit.enabled);
    assert!(config.security.audit.index_to_collection);
}

#[test]
fn test_parse_security_config() {
    let toml_content = r#"
[security]
enabled = true

[[security.api_keys]]
key = "prism_ak_test123"
name = "test-service"
roles = ["analyst"]

[security.roles.analyst]
collections = { "logs-*" = ["read", "search"] }

[security.roles.admin]
collections = { "*" = ["*"] }

[security.audit]
enabled = true
index_to_collection = false
"#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert!(config.security.enabled);
    assert_eq!(config.security.api_keys.len(), 1);
    assert_eq!(config.security.api_keys[0].name, "test-service");
    assert_eq!(config.security.api_keys[0].roles, vec!["analyst"]);
    assert_eq!(config.security.roles.len(), 2);
    assert!(config.security.roles.contains_key("analyst"));
    assert!(config.security.roles.contains_key("admin"));
    assert!(config.security.audit.enabled);
    assert!(!config.security.audit.index_to_collection);
}

#[test]
fn test_parse_config_without_security_section() {
    let toml_content = r#"
[server]
bind_addr = "127.0.0.1:3080"
"#;
    let config: Config = toml::from_str(toml_content).unwrap();
    assert!(!config.security.enabled);
    assert!(config.security.api_keys.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p prism --test config_test test_default_security 2>&1 | tail -5`
Expected: FAIL — no field `security` on `Config`

**Step 3: Add SecurityConfig structs**

In `/home/meeh/prism/prism/src/config/mod.rs`, add after `use std::path::{Path, PathBuf};` (line 12):

```rust
use std::collections::HashMap;
```

After the `Config` struct (after line 29), add `security` field:

```rust
#[serde(default)]
pub security: SecurityConfig,
```

After `TlsConfig` impl Default (after line 119), add:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<ApiKeyConfig>,
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
    #[serde(default)]
    pub audit: AuditConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiKeyConfig {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoleConfig {
    #[serde(default)]
    pub collections: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuditConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub index_to_collection: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_keys: Vec::new(),
            roles: HashMap::new(),
            audit: AuditConfig::default(),
        }
    }
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            index_to_collection: true,
        }
    }
}
```

Update `Config::default()` (around line 193-203) to include:

```rust
security: SecurityConfig::default(),
```

**Step 4: Run all config tests**

Run: `cargo test -p prism --test config_test 2>&1 | tail -15`
Expected: All 13 tests PASS

**Step 5: Commit**

```bash
git add prism/src/config/mod.rs prism/tests/config_test.rs
git commit -m "feat(security): add SecurityConfig with API keys, roles, and audit config"
```

---

### Task 2: Create security module with auth types and permission checker

**Files:**
- Create: `/home/meeh/prism/prism/src/security/mod.rs`
- Create: `/home/meeh/prism/prism/src/security/types.rs`
- Create: `/home/meeh/prism/prism/src/security/permissions.rs`
- Create: `/home/meeh/prism/prism/tests/security_test.rs`
- Modify: `/home/meeh/prism/prism/src/lib.rs` (add `pub mod security;` after line 13)

**Step 1: Write the failing tests**

Create `/home/meeh/prism/prism/tests/security_test.rs`:

```rust
use prism::security::permissions::PermissionChecker;
use prism::security::types::{AuthUser, Permission};
use prism::config::{SecurityConfig, RoleConfig, ApiKeyConfig};
use std::collections::HashMap;

fn test_config() -> SecurityConfig {
    let mut roles = HashMap::new();
    roles.insert("admin".to_string(), RoleConfig {
        collections: HashMap::from([("*".to_string(), vec!["*".to_string()])]),
    });
    roles.insert("analyst".to_string(), RoleConfig {
        collections: HashMap::from([
            ("logs-*".to_string(), vec!["read".to_string(), "search".to_string()]),
            ("metrics-*".to_string(), vec!["read".to_string(), "search".to_string()]),
        ]),
    });
    roles.insert("writer".to_string(), RoleConfig {
        collections: HashMap::from([
            ("products".to_string(), vec!["read".to_string(), "write".to_string(), "delete".to_string()]),
        ]),
    });

    SecurityConfig {
        enabled: true,
        api_keys: vec![
            ApiKeyConfig { key: "prism_ak_admin".to_string(), name: "admin".to_string(), roles: vec!["admin".to_string()] },
            ApiKeyConfig { key: "prism_ak_analyst".to_string(), name: "analyst".to_string(), roles: vec!["analyst".to_string()] },
            ApiKeyConfig { key: "prism_ak_writer".to_string(), name: "writer".to_string(), roles: vec!["writer".to_string()] },
        ],
        roles,
        audit: Default::default(),
    }
}

#[test]
fn test_lookup_api_key() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);

    let user = checker.authenticate("prism_ak_admin").unwrap();
    assert_eq!(user.name, "admin");
    assert_eq!(user.roles, vec!["admin"]);
    assert_eq!(user.key_prefix, "prism_ak_adm...");

    assert!(checker.authenticate("invalid_key").is_none());
}

#[test]
fn test_admin_can_access_everything() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);
    let user = checker.authenticate("prism_ak_admin").unwrap();

    assert!(checker.check_permission(&user, "logs-2026", Permission::Read));
    assert!(checker.check_permission(&user, "products", Permission::Write));
    assert!(checker.check_permission(&user, "anything", Permission::Admin));
}

#[test]
fn test_analyst_read_only_matching_collections() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);
    let user = checker.authenticate("prism_ak_analyst").unwrap();

    // Can read/search matching collections
    assert!(checker.check_permission(&user, "logs-2026", Permission::Read));
    assert!(checker.check_permission(&user, "logs-2026", Permission::Search));
    assert!(checker.check_permission(&user, "metrics-cpu", Permission::Read));

    // Cannot write
    assert!(!checker.check_permission(&user, "logs-2026", Permission::Write));

    // Cannot access non-matching collections
    assert!(!checker.check_permission(&user, "products", Permission::Read));
}

#[test]
fn test_writer_specific_collection() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);
    let user = checker.authenticate("prism_ak_writer").unwrap();

    assert!(checker.check_permission(&user, "products", Permission::Read));
    assert!(checker.check_permission(&user, "products", Permission::Write));
    assert!(checker.check_permission(&user, "products", Permission::Delete));

    // Cannot access other collections
    assert!(!checker.check_permission(&user, "logs-2026", Permission::Read));
}

#[test]
fn test_glob_pattern_matching() {
    let config = test_config();
    let checker = PermissionChecker::new(&config);

    // logs-* should match logs-anything
    let user = checker.authenticate("prism_ak_analyst").unwrap();
    assert!(checker.check_permission(&user, "logs-production", Permission::Read));
    assert!(checker.check_permission(&user, "logs-", Permission::Read));
    assert!(!checker.check_permission(&user, "logs", Permission::Read)); // no dash, no match
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p prism --test security_test 2>&1 | tail -5`
Expected: FAIL — module `security` not found

**Step 3: Create the security module**

Create `/home/meeh/prism/prism/src/security/mod.rs`:

```rust
pub mod permissions;
pub mod types;
```

Create `/home/meeh/prism/prism/src/security/types.rs`:

```rust
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub name: String,
    pub roles: Vec<String>,
    pub key_prefix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Read,
    Write,
    Delete,
    Search,
    Admin,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Read => "read",
            Permission::Write => "write",
            Permission::Delete => "delete",
            Permission::Search => "search",
            Permission::Admin => "admin",
        }
    }
}
```

Create `/home/meeh/prism/prism/src/security/permissions.rs`:

```rust
use crate::config::SecurityConfig;
use super::types::{AuthUser, Permission};
use std::collections::HashMap;

pub struct PermissionChecker {
    /// API key -> (name, roles, key_prefix)
    keys: HashMap<String, (String, Vec<String>)>,
    /// Role name -> collection patterns -> permissions
    roles: HashMap<String, Vec<(String, Vec<String>)>>,
}

impl PermissionChecker {
    pub fn new(config: &SecurityConfig) -> Self {
        let keys: HashMap<String, (String, Vec<String>)> = config
            .api_keys
            .iter()
            .map(|ak| (ak.key.clone(), (ak.name.clone(), ak.roles.clone())))
            .collect();

        let roles: HashMap<String, Vec<(String, Vec<String>)>> = config
            .roles
            .iter()
            .map(|(name, role_config)| {
                let patterns: Vec<(String, Vec<String>)> = role_config
                    .collections
                    .iter()
                    .map(|(pat, perms)| (pat.clone(), perms.clone()))
                    .collect();
                (name.clone(), patterns)
            })
            .collect();

        Self { keys, roles }
    }

    pub fn authenticate(&self, api_key: &str) -> Option<AuthUser> {
        self.keys.get(api_key).map(|(name, roles)| {
            let prefix = if api_key.len() > 13 {
                format!("{}...", &api_key[..13])
            } else {
                api_key.to_string()
            };
            AuthUser {
                name: name.clone(),
                roles: roles.clone(),
                key_prefix: prefix,
            }
        })
    }

    pub fn check_permission(&self, user: &AuthUser, collection: &str, permission: Permission) -> bool {
        for role_name in &user.roles {
            if let Some(patterns) = self.roles.get(role_name) {
                for (pattern, perms) in patterns {
                    if glob_match(pattern, collection) {
                        if perms.iter().any(|p| p == "*" || p == permission.as_str()) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

/// Simple glob matching: only supports trailing `*` (e.g., `logs-*`, `*`)
fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}
```

Add to `/home/meeh/prism/prism/src/lib.rs` after line 13 (`pub mod storage;`):

```rust
pub mod security;
```

**Step 4: Run all security tests**

Run: `cargo test -p prism --test security_test 2>&1 | tail -15`
Expected: All 5 tests PASS

**Step 5: Commit**

```bash
git add prism/src/security/ prism/src/lib.rs prism/tests/security_test.rs
git commit -m "feat(security): add auth types and permission checker with glob patterns"
```

---

### Task 3: Add auth middleware

**Files:**
- Create: `/home/meeh/prism/prism/src/security/middleware.rs`
- Modify: `/home/meeh/prism/prism/src/security/mod.rs` (add `pub mod middleware;`)

**Step 1: Create the auth middleware**

Create `/home/meeh/prism/prism/src/security/middleware.rs`:

```rust
use axum::{
    body::Body,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

use super::permissions::PermissionChecker;
use super::types::{AuthUser, Permission};

/// Routes that do not require authentication
const AUTH_WHITELIST: &[&str] = &["/health", "/stats/server"];

/// Extract collection name from URL path
fn extract_collection(path: &str) -> Option<&str> {
    // Match /collections/:collection/...
    let path = path.strip_prefix("/collections/")?;
    path.split('/').next()
}

/// Determine required permission from HTTP method and path
fn required_permission(method: &axum::http::Method, path: &str) -> Permission {
    if path.starts_with("/admin/") {
        return Permission::Admin;
    }

    match *method {
        axum::http::Method::GET => Permission::Read,
        axum::http::Method::POST => {
            if path.contains("/search") || path.contains("/_suggest") || path.contains("/_mlt") || path.contains("/aggregate") {
                Permission::Search
            } else {
                Permission::Write
            }
        }
        axum::http::Method::DELETE => Permission::Delete,
        axum::http::Method::PUT => Permission::Write,
        _ => Permission::Read,
    }
}

pub async fn auth_middleware(
    checker: Arc<PermissionChecker>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    // Skip auth for whitelisted routes
    if AUTH_WHITELIST.iter().any(|w| path.starts_with(w)) {
        return Ok(next.run(request).await);
    }

    // Extract API key from Authorization header
    let api_key = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let api_key = match api_key {
        Some(k) => k,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Authenticate
    let user = match checker.authenticate(api_key) {
        Some(u) => u,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    // Check collection-level permission
    if let Some(collection) = extract_collection(&path) {
        let perm = required_permission(&method, &path);
        if !checker.check_permission(&user, collection, perm) {
            return Err(StatusCode::FORBIDDEN);
        }
    } else if path.starts_with("/admin/") {
        if !checker.check_permission(&user, "*", Permission::Admin) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    // Store AuthUser in request extensions for handlers
    let mut request = request;
    request.extensions_mut().insert(user);

    Ok(next.run(request).await)
}
```

Update `/home/meeh/prism/prism/src/security/mod.rs`:

```rust
pub mod middleware;
pub mod permissions;
pub mod types;
```

**Step 2: Verify compilation**

Run: `cargo check -p prism 2>&1 | tail -5`
Expected: compiles clean

**Step 3: Commit**

```bash
git add prism/src/security/middleware.rs prism/src/security/mod.rs
git commit -m "feat(security): add auth middleware with route whitelist and permission checks"
```

---

### Task 4: Add audit middleware

**Files:**
- Create: `/home/meeh/prism/prism/src/security/audit.rs`
- Modify: `/home/meeh/prism/prism/src/security/mod.rs` (add `pub mod audit;`)

**Step 1: Create the audit middleware**

Create `/home/meeh/prism/prism/src/security/audit.rs`:

```rust
use axum::{
    body::Body,
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
```

Update `/home/meeh/prism/prism/src/security/mod.rs`:

```rust
pub mod audit;
pub mod middleware;
pub mod permissions;
pub mod types;
```

**Step 2: Verify compilation**

Run: `cargo check -p prism 2>&1 | tail -5`
Expected: compiles clean

**Step 3: Commit**

```bash
git add prism/src/security/audit.rs prism/src/security/mod.rs
git commit -m "feat(security): add audit middleware with tracing and collection indexing"
```

---

### Task 5: Wire middleware into ApiServer

**Files:**
- Modify: `/home/meeh/prism/prism/src/api/server.rs` (lines 33-58 ApiServer struct, lines 171-267 router method)
- Modify: `/home/meeh/prism/prism/src/config/mod.rs` (export SecurityConfig types if needed)

**Step 1: Update ApiServer to accept SecurityConfig**

In `/home/meeh/prism/prism/src/api/server.rs`:

Add import at top:

```rust
use crate::config::SecurityConfig;
use crate::security::permissions::PermissionChecker;
```

Add `security_config` field to `ApiServer`:

```rust
pub struct ApiServer {
    manager: Arc<CollectionManager>,
    session_manager: Arc<SessionManager>,
    mcp_handler: Arc<McpHandler>,
    cors_config: CorsConfig,
    security_config: SecurityConfig,
}
```

Add a new constructor `with_security`:

```rust
pub fn with_security(
    manager: Arc<CollectionManager>,
    cors_config: CorsConfig,
    security_config: SecurityConfig,
) -> Self {
    let session_manager = Arc::new(SessionManager::new());
    let mut tool_registry = ToolRegistry::new();
    register_basic_tools(&mut tool_registry);
    let tool_registry = Arc::new(tool_registry);
    let mcp_handler = Arc::new(McpHandler::new(tool_registry, manager.clone()));

    Self {
        manager,
        session_manager,
        mcp_handler,
        cors_config,
        security_config,
    }
}
```

Update existing `new` and `with_cors` to pass default SecurityConfig.

**Step 2: Add middleware layers to router()**

In the `router()` method, after CORS and trace layers (around line 262-266), conditionally add auth and audit layers:

```rust
let mut app = Router::new()
    .merge(legacy_routes)
    .merge(mcp_routes)
    .layer(cors)
    .layer(TraceLayer::new_for_http());

// Add audit middleware (independent of security.enabled)
if self.security_config.audit.enabled {
    let mgr = self.manager.clone();
    let index = self.security_config.audit.index_to_collection;
    app = app.layer(axum::middleware::from_fn(move |req, next| {
        crate::security::audit::audit_middleware(mgr.clone(), index, req, next)
    }));
}

// Add auth middleware (only when security.enabled)
if self.security_config.enabled {
    let checker = Arc::new(PermissionChecker::new(&self.security_config));
    app = app.layer(axum::middleware::from_fn(move |req, next| {
        crate::security::middleware::auth_middleware(checker.clone(), req, next)
    }));
}

app
```

**Note on layer ordering:** Auth layer is added last so it runs first (tower layers are LIFO). This means: auth → audit → cors → trace → handler.

**Step 3: Update prism-server main.rs**

In `/home/meeh/prism/prism-server/src/main.rs`, change:

```rust
let server = prism::api::ApiServer::with_cors(manager, config.server.cors.clone());
```

To:

```rust
let server = prism::api::ApiServer::with_security(
    manager,
    config.server.cors.clone(),
    config.security.clone(),
);
```

**Step 4: Verify compilation**

Run: `cargo check -p prism-server 2>&1 | tail -5`
Expected: compiles clean

**Step 5: Commit**

```bash
git add prism/src/api/server.rs prism-server/src/main.rs
git commit -m "feat(security): wire auth and audit middleware into ApiServer"
```

---

### Task 6: Add Error::Unauthorized and Error::Forbidden variants

**Files:**
- Modify: `/home/meeh/prism/prism/src/error.rs` (add variants)

**Step 1: Add error variants**

In `/home/meeh/prism/prism/src/error.rs`, add after `Config(String)` (line 36):

```rust
#[error("Unauthorized: {0}")]
Unauthorized(String),

#[error("Forbidden: {0}")]
Forbidden(String),
```

**Step 2: Verify compilation**

Run: `cargo check -p prism 2>&1 | tail -5`
Expected: compiles clean

**Step 3: Commit**

```bash
git add prism/src/error.rs
git commit -m "feat(security): add Unauthorized and Forbidden error variants"
```

---

### Task 7: Update xtask dist with security config template

**Files:**
- Modify: `/home/meeh/prism/xtask/src/main.rs` (generate_prism_toml function)

**Step 1: Add security section to generated prism.toml**

In the `generate_prism_toml()` function, add after the `[server.tls]` section:

```toml

[security]
# Enable API key authentication and RBAC
# enabled = true

# [[security.api_keys]]
# key = "prism_ak_change_me_to_a_random_string"
# name = "default-admin"
# roles = ["admin"]

# [security.roles.admin]
# collections = { "*" = ["*"] }

[security.audit]
# Enable audit logging
enabled = false
index_to_collection = true
```

**Step 2: Verify xtask compiles**

Run: `cargo check -p xtask 2>&1 | tail -5`
Expected: compiles clean

**Step 3: Commit**

```bash
git add xtask/src/main.rs
git commit -m "feat(security): add security config template to dist bundle"
```

---

### Task 8: Integration tests

**Files:**
- Create: `/home/meeh/prism/prism/tests/security_integration_test.rs`

**Step 1: Write integration tests**

Create `/home/meeh/prism/prism/tests/security_integration_test.rs`:

```rust
//! Integration tests for security middleware
//!
//! These tests start a real HTTP server with security enabled and verify
//! auth + audit behavior end-to-end.

use prism::api::ApiServer;
use prism::backends::text::TextBackend;
use prism::backends::VectorBackend;
use prism::collection::CollectionManager;
use prism::config::{SecurityConfig, ApiKeyConfig, RoleConfig, AuditConfig, CorsConfig};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};

async fn setup_server(security: SecurityConfig) -> (TempDir, String) {
    let temp = TempDir::new().unwrap();
    let schemas_dir = temp.path().join("schemas");
    std::fs::create_dir_all(&schemas_dir).unwrap();

    let text_backend = Arc::new(TextBackend::new(temp.path()).unwrap());
    let vector_backend = Arc::new(VectorBackend::new(temp.path()).unwrap());
    let manager = Arc::new(
        CollectionManager::new(&schemas_dir, text_backend, vector_backend).unwrap(),
    );
    manager.initialize().await.unwrap();

    let server = ApiServer::with_security(manager, CorsConfig::default(), security);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, server.router()).await.unwrap();
    });

    sleep(Duration::from_millis(50)).await;
    (temp, url)
}

fn security_config() -> SecurityConfig {
    let mut roles = HashMap::new();
    roles.insert("admin".to_string(), RoleConfig {
        collections: HashMap::from([("*".to_string(), vec!["*".to_string()])]),
    });
    roles.insert("reader".to_string(), RoleConfig {
        collections: HashMap::from([("test-*".to_string(), vec!["read".to_string(), "search".to_string()])]),
    });

    SecurityConfig {
        enabled: true,
        api_keys: vec![
            ApiKeyConfig { key: "test_admin_key".to_string(), name: "admin".to_string(), roles: vec!["admin".to_string()] },
            ApiKeyConfig { key: "test_reader_key".to_string(), name: "reader".to_string(), roles: vec!["reader".to_string()] },
        ],
        roles,
        audit: AuditConfig { enabled: false, index_to_collection: false },
    }
}

#[tokio::test]
async fn test_health_no_auth_required() {
    let (_temp, url) = setup_server(security_config()).await;
    let resp = reqwest::get(format!("{}/health", url)).await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_missing_api_key_returns_401() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{}/admin/collections", url)).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_invalid_api_key_returns_401() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/collections", url))
        .header("Authorization", "Bearer bad_key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_valid_admin_key_returns_200() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/collections", url))
        .header("Authorization", "Bearer test_admin_key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_reader_cannot_access_admin() {
    let (_temp, url) = setup_server(security_config()).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/collections", url))
        .header("Authorization", "Bearer test_reader_key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_security_disabled_allows_all() {
    let disabled = SecurityConfig::default(); // enabled = false
    let (_temp, url) = setup_server(disabled).await;
    let client = reqwest::Client::new();
    // No auth header, should work
    let resp = client.get(format!("{}/admin/collections", url)).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}
```

**Step 2: Run integration tests**

Run: `cargo test -p prism --test security_integration_test 2>&1 | tail -15`
Expected: All 6 tests PASS

**Step 3: Also run all existing tests to verify no regressions**

Run: `cargo test -p prism --test config_test --test security_test 2>&1 | tail -15`
Expected: All tests PASS

**Step 4: Commit**

```bash
git add prism/tests/security_integration_test.rs
git commit -m "feat(security): add integration tests for auth middleware"
```

---

### Task 9: Final commit

**Step 1: Run full test suite**

Run: `cargo test -p prism 2>&1 | tail -20`
Expected: All tests pass

**Step 2: Final commit closing the issue**

```bash
git add -A
git commit -m "feat(security): API key auth, RBAC, and audit logging (closes #49)"
```
