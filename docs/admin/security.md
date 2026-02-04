# Security Configuration

Prism supports API key authentication with role-based access control (RBAC) and optional audit logging.

## Enabling Security

```toml
[security]
enabled = true
```

When enabled, all requests must include a valid API key in the `Authorization` header:

```bash
curl -H "Authorization: Bearer sk-abc123..." http://localhost:3080/collections/docs/search
```

## API Keys

Define API keys with associated roles:

```toml
[[security.api_keys]]
key = "sk-abc123def456"
name = "admin-key"
roles = ["admin"]

[[security.api_keys]]
key = "sk-reader789xyz"
name = "readonly-key"
roles = ["reader"]

[[security.api_keys]]
key = "sk-writer456abc"
name = "indexer-key"
roles = ["writer"]
```

| Field | Required | Description |
|-------|----------|-------------|
| `key` | Yes | The API key value (keep secret!) |
| `name` | Yes | Human-readable identifier |
| `roles` | Yes | List of role names |

### Key Generation

Generate secure API keys:

```bash
# Using openssl
openssl rand -hex 32 | sed 's/^/sk-/'

# Using /dev/urandom
head -c 32 /dev/urandom | xxd -p | sed 's/^/sk-/'
```

## Role-Based Access Control

Define what each role can do per collection:

```toml
# Admin: full access to everything
[security.roles.admin.collections]
"*" = ["read", "write", "delete", "admin"]

# Reader: read-only access to all collections
[security.roles.reader.collections]
"*" = ["read"]

# Writer: read/write to public collections only
[security.roles.writer.collections]
"public-*" = ["read", "write"]
"*" = ["read"]

# Restricted: deny specific collections
[security.roles.restricted.collections]
"*" = ["read"]
"internal-*" = []        # explicit deny
"private-*" = []         # explicit deny
```

### Available Permissions

| Permission | Allows |
|------------|--------|
| `read` | Search, get documents, view stats |
| `write` | Index documents, update documents |
| `delete` | Delete documents |
| `admin` | Manage collection settings, optimize |

### Pattern Matching

Collection patterns support wildcards:

| Pattern | Matches |
|---------|---------|
| `*` | All collections |
| `public-*` | `public-docs`, `public-articles`, etc. |
| `logs-2024-*` | `logs-2024-01`, `logs-2024-02`, etc. |

Rules are evaluated in order; first match wins. Use explicit empty array `[]` to deny access.

## Audit Logging

Track all API requests for security and compliance:

```toml
[security.audit]
enabled = true
index_to_collection = true
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `false` | Enable audit logging |
| `index_to_collection` | `false` | Index audit logs to `_audit` collection |

### Audit Log Fields

When `index_to_collection = true`, each request is logged with:

| Field | Description |
|-------|-------------|
| `timestamp` | Request time (ISO 8601) |
| `api_key_name` | Name of API key used |
| `method` | HTTP method |
| `path` | Request path |
| `collection` | Target collection (if applicable) |
| `status` | Response status code |
| `duration_ms` | Request duration |
| `client_ip` | Client IP address |

Query audit logs:

```bash
curl -X POST http://localhost:3080/collections/_audit/search \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "*",
    "filters": {
      "api_key_name": "indexer-key"
    },
    "limit": 100
  }'
```

## Example: Multi-Tenant Setup

```toml
[security]
enabled = true

# Tenant A
[[security.api_keys]]
key = "sk-tenant-a-abc123"
name = "tenant-a"
roles = ["tenant-a"]

# Tenant B
[[security.api_keys]]
key = "sk-tenant-b-xyz789"
name = "tenant-b"
roles = ["tenant-b"]

# Each tenant only sees their collections
[security.roles.tenant-a.collections]
"tenant-a-*" = ["read", "write", "delete"]

[security.roles.tenant-b.collections]
"tenant-b-*" = ["read", "write", "delete"]
```

## Security Best Practices

1. **Never commit API keys** — Use environment variables or secrets management
2. **Rotate keys regularly** — Generate new keys and update clients
3. **Use least privilege** — Grant minimum necessary permissions
4. **Enable audit logging** — Track access for compliance
5. **Use TLS in production** — Encrypt traffic with HTTPS

## See Also

- [Configuration](configuration.md) — Full config reference
- [Deployment](deployment.md) — Production setup with TLS
