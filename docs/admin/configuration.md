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
| `cors.enabled` | `true` | Enable CORS headers |
| `cors.origins` | dev ports | Allowed origins |
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
| `data_dir` | `~/.prismsearch` | Base directory for all data |
| `max_local_gb` | — | Maximum local storage in GB |

For advanced storage options (S3, caching), see [Storage Backends](storage-backends.md).

### Embedding Settings

```toml
[embedding]
enabled = true
batch_size = 128       # Max texts per embedding API call
concurrency = 4        # Max concurrent embedding API calls
cache_dir = "~/.prism/cache/embeddings.db"

[embedding.provider]
type = "ollama"
url = "http://localhost:11434"
model = "nomic-embed-text"
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `true` | Enable embedding generation |
| `batch_size` | `128` | Max texts per embedding API call |
| `concurrency` | `4` | Max concurrent embedding API calls |
| `cache_dir` | — | Embedding cache path (auto-detected if omitted) |
| `provider.type` | `ollama` | Provider: `ollama`, `openai`, or `onnx` |
| `provider.url` | `http://localhost:11434` | Provider API URL |
| `provider.model` | `nomic-embed-text` | Embedding model name |
| `provider.api_key` | — | API key (OpenAI provider only) |

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
| `metrics_enabled` | `true` | Expose Prometheus metrics |

See [Monitoring](monitoring.md) for metrics details.

### Security Settings

```toml
[security]
enabled = false
```

See [Security](security.md) for API keys, RBAC, and audit logging.

### Cluster Settings

Requires the `cluster` feature flag at build time.

```toml
[cluster]
enabled = true
node_id = "node-1"
bind_addr = "0.0.0.0:9080"
advertise_addr = "prism-node1:9080"
seed_nodes = ["prism-node2:9080", "prism-node3:9080"]
connect_timeout_ms = 5000
request_timeout_ms = 30000

[cluster.tls]
enabled = true
cert_path = "/conf/tls/node-cert.pem"
key_path = "/conf/tls/node-key.pem"
ca_cert_path = "/conf/tls/ca-cert.pem"
skip_verify = false
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `false` | Enable cluster mode |
| `node_id` | random UUID | Unique node identifier |
| `bind_addr` | `0.0.0.0:9080` | QUIC RPC bind address (UDP) |
| `advertise_addr` | same as `bind_addr` | Reachable address advertised to peers |
| `seed_nodes` | `[]` | Other cluster nodes to connect to |
| `connect_timeout_ms` | `5000` | QUIC connection timeout |
| `request_timeout_ms` | `30000` | RPC request timeout |
| `tls.enabled` | `true` | Enable TLS for inter-node traffic |
| `tls.cert_path` | `./conf/tls/cluster-cert.pem` | Node certificate (PEM) |
| `tls.key_path` | `./conf/tls/cluster-key.pem` | Node private key (PEM) |
| `tls.ca_cert_path` | — | CA certificate for peer verification |
| `tls.skip_verify` | `false` | Skip peer verification (dev only) |

See [Clustering & Federation](../guides/clustering.md) for the full guide.

### Background Optimize

Prism uses `NoMergePolicy` by default, which means Tantivy segments accumulate over time. The background optimize service periodically merges segments across all collections to keep search latency low.

```toml
[optimize]
enabled = true
interval_secs = 3600       # Run every hour
max_segments = 5            # Target max segments per collection
max_segment_size = "1GB"    # Don't merge beyond this segment size
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `false` | Enable background segment merging |
| `interval_secs` | `3600` | Seconds between optimize cycles |
| `max_segments` | `5` | Target max segments per collection |
| `max_segment_size` | — | Max segment size (e.g. `"1GB"`, `"500MB"`). When set, the effective segment count is raised so no merged segment exceeds this size |

Collections already at or below the target segment count are skipped. When `max_segment_size` is set and the total index data exceeds `max_segments * max_segment_size`, the optimizer will keep more segments to respect the size cap.

You can also trigger a one-off merge via the API:

```bash
curl -X POST http://localhost:3080/collections/my-collection/optimize
```

### Index Lifecycle Management (ILM)

```toml
[ilm]
enabled = true
check_interval_secs = 60

[ilm.policies.logs]
description = "Log retention policy"
rollover_max_size = "50GB"
rollover_max_age = "1d"

[ilm.policies.logs.phases.warm]
min_age = "7d"
readonly = true

[ilm.policies.logs.phases.delete]
min_age = "90d"
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `false` | Enable ILM processing |
| `check_interval_secs` | `60` | Seconds between ILM policy checks |
| `policies.<name>.rollover_max_size` | — | Rollover threshold (e.g. `"50GB"`) |
| `policies.<name>.rollover_max_age` | — | Rollover age (e.g. `"1d"`, `"7d"`) |
| `policies.<name>.rollover_max_docs` | — | Rollover document count |
| `policies.<name>.phases.<phase>.min_age` | `"0d"` | Minimum age to enter phase |
| `policies.<name>.phases.<phase>.readonly` | `false` | Make index read-only |
| `policies.<name>.phases.<phase>.storage` | — | Storage tier: `"local"` or `"s3"` |
| `policies.<name>.phases.<phase>.force_merge_segments` | — | Force merge to N segments |

Phases: `hot`, `warm`, `cold`, `frozen`, `delete`.

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
