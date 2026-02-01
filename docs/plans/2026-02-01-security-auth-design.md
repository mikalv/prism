# Security: API Keys, RBAC, and Audit Logging — Design

**Issue:** #49 (first iteration)
**Date:** 2026-02-01

## Scope

First iteration of security for Prism:
- API key authentication
- Collection-level RBAC with glob patterns
- Audit logging to tracing + self-indexed `_audit` collection

Out of scope for this iteration: JWT, OIDC, mTLS, field-level security, document-level security.

## Decisions

- API keys stored in `prism.toml` config
- Flat RBAC: collection-level permissions with glob patterns
- Auth via axum tower middleware layer with route whitelist
- Audit logging to both tracing and internal `_audit` collection
- `security.enabled = false` by default — no middleware, fully open
- `security.audit.enabled` independent of `security.enabled`

## Config

```toml
[security]
enabled = false

[[security.api_keys]]
key = "prism_ak_abc123def456"
name = "analytics-service"
roles = ["analyst"]

[[security.api_keys]]
key = "prism_ak_admin_xyz789"
name = "admin-user"
roles = ["admin"]

[security.roles.admin]
collections = { "*" = ["*"] }

[security.roles.analyst]
collections = { "logs-*" = ["read", "search"], "metrics-*" = ["read", "search"] }

[security.roles.writer]
collections = { "products" = ["read", "write", "delete"] }

[security.audit]
enabled = false
index_to_collection = true
```

Permissions: `read`, `write`, `delete`, `search`, `admin`, `*` (all).
Glob patterns on collection names: `logs-*`, `*`.

## Architecture

### Request flow (security.enabled = true)

```
Request → AuthMiddleware → Route Handler → Response
              │
              ├─ Read Authorization: Bearer <key> header
              ├─ Look up key in HashMap (built from config at startup)
              ├─ Missing/invalid key → 401 Unauthorized
              ├─ Valid key → set AuthUser in request extensions
              ├─ Check role access against collection + action
              ├─ No access → 403 Forbidden
              └─ OK → continue to handler
```

Whitelist (no auth required): `/health`, `/stats/server`.

### AuthUser

```rust
pub struct AuthUser {
    pub name: String,
    pub roles: Vec<String>,
    pub key_prefix: String,  // for logging, never full key
}
```

### Permission check

Middleware parses collection name from URL path (`/collections/:collection/*`).
Checks if any of the user's roles grant access to that collection with the required action.
Admin routes (`/admin/*`) require `admin` role.

### Audit logging

Every request (except `/health`) generates an audit event:

```rust
pub struct AuditEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,      // "search", "index", "delete", "admin"
    pub user: Option<String>,    // None when security.enabled = false
    pub roles: Vec<String>,
    pub collection: Option<String>,
    pub action: String,          // "POST /collections/logs/search"
    pub status_code: u16,
    pub client_ip: String,
    pub duration_ms: u64,
}
```

Dual output:
1. `tracing::info!(target: "prism::audit", ...)` — always
2. Index to `_audit` collection — async fire-and-forget via `tokio::spawn`, fails silently (warning logged)

### `_audit` collection

Auto-created at startup when `security.audit.index_to_collection = true`. Hardcoded schema:

- timestamp (date, indexed)
- event_type (string, indexed)
- user (string, indexed)
- collection (string, indexed)
- action (text, stored)
- status_code (u64, indexed)
- client_ip (string, indexed)
- duration_ms (u64, indexed)

Searchable via `POST /collections/_audit/search`.
Protected: requires `admin` role when security is enabled.

## Implementation order

1. SecurityConfig structs (config/mod.rs)
2. Auth types: AuthUser, Permission, Role, AuditEvent (new security module)
3. Permission checker: glob matching, role resolution
4. Auth middleware layer
5. Audit middleware layer (independent of auth)
6. `_audit` collection auto-creation and indexing
7. Wire middleware into ApiServer
8. Integration tests
