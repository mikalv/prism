# Configuration Reference

Prism reads configuration from a TOML file. By default it looks for `prism.toml` in the working directory, or specify with `-c /path/to/config.toml`.

## Minimal Configuration

```toml
[server]
bind_addr = "127.0.0.1:3080"

[storage]
data_dir = "~/.prism"

[logging]
level = "info"
```

## Full Configuration Reference

### Server Settings

```toml
[server]
bind_addr = "127.0.0.1:3080"   # Host:port to listen on
unix_socket = "/var/run/prism.sock"  # Optional Unix socket

[server.cors]
enabled = true
origins = [
  "http://localhost:5173",
  "http://localhost:3000"
]

[server.tls]
enabled = false
bind_addr = "127.0.0.1:3443"
cert_path = "./conf/tls/cert.pem"
key_path = "./conf/tls/key.pem"
```

| Option | Default | Description |
|--------|---------|-------------|
| `bind_addr` | `127.0.0.1:8080` | Address and port to bind |
| `unix_socket` | — | Optional Unix socket path |
| `cors.enabled` | `false` | Enable CORS headers |
| `cors.origins` | `[]` | Allowed origins |
| `tls.enabled` | `false` | Enable TLS |
| `tls.bind_addr` | — | TLS bind address |
| `tls.cert_path` | — | Path to TLS certificate |
| `tls.key_path` | — | Path to TLS private key |

### Storage Settings

```toml
[storage]
data_dir = "~/.prism"
max_local_gb = 5.0
```

| Option | Default | Description |
|--------|---------|-------------|
| `data_dir` | `~/.engraph` | Base directory for all data |
| `max_local_gb` | — | Maximum local storage in GB |

For advanced storage options (S3, caching), see [Storage Backends](storage-backends.md).

### Embedding Settings

```toml
[embedding]
enabled = true
model = "all-MiniLM-L6-v2"
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `true` | Enable embedding generation |
| `model` | `all-MiniLM-L6-v2` | Model name for local embeddings |

### Logging Settings

```toml
[logging]
level = "info"
file = "/var/log/prism.log"
```

| Option | Default | Description |
|--------|---------|-------------|
| `level` | `info` | Log level: `debug`, `info`, `warn`, `error` |
| `file` | — | Log file path (stdout if omitted) |

### Observability Settings

```toml
[observability]
log_format = "pretty"
log_level = "info,prism=debug"
metrics_enabled = true
```

| Option | Default | Description |
|--------|---------|-------------|
| `log_format` | `pretty` | Format: `pretty` or `json` |
| `log_level` | `info` | Rust-style log filter |
| `metrics_enabled` | `false` | Expose Prometheus metrics |

See [Monitoring](monitoring.md) for metrics details.

### Security Settings

```toml
[security]
enabled = false
```

See [Security](security.md) for API keys, RBAC, and audit logging.

## Environment Variables

Prism supports configuration via environment variables. These override CLI flags and config file settings.

### Server Configuration

| Variable | CLI Flag | Default | Description |
|----------|----------|---------|-------------|
| `PRISM_CONFIG_PATH` | `-c, --config` | `prism.toml` | Configuration file path |
| `PRISM_HOST` | `--host` | `127.0.0.1` | Bind host address |
| `PRISM_PORT` | `-p, --port` | `3080` | Listen port |
| `PRISM_DATA_DIR` | `--data-dir` | `data` | Data directory |
| `PRISM_SCHEMAS_DIR` | `--schemas-dir` | `schemas` | Schemas directory |
| `PRISM_LOG_DIR` | `--log-dir` | — | Logs directory (optional) |
| `PRISM_CACHE_DIR` | `--cache-dir` | — | Embedding cache directory |

### Logging Configuration

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Override log level (e.g., `info,prism=debug`) |
| `LOG_FORMAT` | Override log format (`pretty` or `json`) |

### Priority Order

Configuration is resolved in this order (highest priority first):

1. **CLI arguments** — `prism-server --port 8080`
2. **Environment variables** — `PRISM_PORT=8080`
3. **Config file** — `prism.toml`
4. **Defaults**

### Example: Docker Environment

```yaml
environment:
  - PRISM_CONFIG_PATH=/etc/prism/prism.toml
  - PRISM_DATA_DIR=/var/lib/prism/data
  - PRISM_LOG_DIR=/var/log/prism
  - PRISM_CACHE_DIR=/var/cache/prism/embeddings
  - RUST_LOG=info,prism=debug
  - LOG_FORMAT=json
```

### Example: Systemd Service

```ini
[Service]
Environment="PRISM_CONFIG_PATH=/etc/prism/prism.toml"
Environment="PRISM_DATA_DIR=/var/lib/prism/data"
Environment="PRISM_LOG_DIR=/var/log/prism"
Environment="PRISM_CACHE_DIR=/var/cache/prism"
Environment="RUST_LOG=info"
Environment="LOG_FORMAT=json"
ExecStart=/usr/local/bin/prism-server
```

## Directory Layout

When Prism starts, it creates this structure under `data_dir`:

```
~/.prism/
├── schemas/            # Collection schema YAML files
│   ├── articles.yaml
│   └── products.yaml
├── data/
│   ├── text/           # Tantivy indexes (one per collection)
│   └── vector/         # HNSW vector indexes
├── cache/
│   └── models/         # Embedding model cache
└── logs/               # Log files (if configured)
```

## See Also

- [Storage Backends](storage-backends.md) — S3, MinIO, cached storage
- [Security](security.md) — API keys and RBAC
- [Monitoring](monitoring.md) — Prometheus metrics
- [Deployment](deployment.md) — Docker and production setup
