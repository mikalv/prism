# Prism Codebase Analysis

> Comprehensive analysis for user/sysadmin documentation

---

## 1. Project Overview

**Prism** is a high-performance hybrid search engine combining vector search (HNSW) with full-text search (Tantivy) for AI/RAG applications.

| Attribute | Value |
|-----------|-------|
| **Type** | Search engine library + HTTP server |
| **Language** | Rust (2021 edition) |
| **Version** | 0.3.0 |
| **License** | MIT |

### Workspace Crates

| Crate | Purpose |
|-------|---------|
| `prism` | Core library with backends, embedding, schema, search |
| `prism-server` | HTTP server (Axum-based REST API) |
| `prism-cli` | Command-line tools for management |
| `prism-importer` | Import from Elasticsearch and other sources |
| `prism-storage` | Storage abstraction (local, S3, cached) |
| `xtask` | Build/dev automation tasks |

---

## 2. CLI Reference

### prism-server

HTTP server for Prism.

```bash
prism-server [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-c, --config <FILE>` | `prism.toml` | Configuration file path |
| `--host <HOST>` | `127.0.0.1` | Host to bind to |
| `-p, --port <PORT>` | `3080` | Port to listen on |

**Environment variables:**
- `RUST_LOG` — Override log level (e.g., `info,prism=debug`)
- `LOG_FORMAT` — Override log format (`pretty` or `json`)

---

### prism (CLI)

Management tools for Prism.

```bash
prism [OPTIONS] <COMMAND>
```

**Global options:**
| Option | Default | Description |
|--------|---------|-------------|
| `-d, --data-dir <DIR>` | `./data` | Data directory |

#### Commands

##### collection inspect
```bash
prism collection inspect --name <NAME> [--verbose]
```
Inspect collection index structure and statistics.

##### collection list
```bash
prism collection list
```
List all collections.

##### document import
```bash
prism document import --collection <NAME> [OPTIONS]
```
| Option | Default | Description |
|--------|---------|-------------|
| `-c, --collection` | required | Collection name |
| `-f, --file <FILE>` | stdin | JSONL input file |
| `--api-url` | `http://localhost:3080` | Prism API URL |
| `--batch-size` | `100` | Documents per batch |
| `--no-progress` | false | Disable progress output |

##### document export
```bash
prism document export --collection <NAME> [--output <FILE>]
```
Export collection to JSONL.

##### index optimize
```bash
prism index optimize --collection <NAME> [--gc-only]
```
Merge segments and garbage collect.

##### benchmark
```bash
prism benchmark --collection <NAME> --queries <FILE> [OPTIONS]
```
| Option | Default | Description |
|--------|---------|-------------|
| `-r, --repeat` | `10` | Repeat each query N times |
| `-w, --warmup` | `3` | Warmup iterations |
| `-k, --top-k` | `10` | Results to fetch |

##### cache-stats
```bash
prism cache-stats --path <PATH>
```
Show embedding cache statistics.

##### cache-clear
```bash
prism cache-clear --path <PATH> [--older-than-days <N>]
```
Clear embedding cache.

---

### prism-import

Import data from external search engines.

```bash
prism-import <COMMAND>
```

#### es (Elasticsearch import)
```bash
prism-import es --source <URL> --index <NAME> [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--source` | required | Elasticsearch URL (e.g., `http://localhost:9200`) |
| `--index` | required | Index name or pattern |
| `--target` | index name | Target Prism collection name |
| `--user` | — | Username for basic auth |
| `--password` | — | Password for basic auth |
| `--api-key` | — | API key for authentication |
| `--batch-size` | `1000` | Scroll API batch size |
| `--dry-run` | false | Only show schema, don't import |
| `--schema-out <FILE>` | — | Output schema to YAML file |

**Authentication methods:**
1. None (default)
2. Basic auth: `--user <USER> --password <PASS>`
3. API key: `--api-key <KEY>`

---

## 3. Configuration Reference

### Main Configuration File (`prism.toml`)

```toml
# Server settings
[server]
bind_addr = "127.0.0.1:8080"
unix_socket = "/var/run/prism.sock"  # optional

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

# Storage settings
[storage]
data_dir = "~/.engraph"
max_local_gb = 5.0

# Unified storage (overrides basic storage if present)
[unified_storage]
backend = "local"  # "local", "s3", or "cached"
data_dir = "~/.prism/data"
buffer_dir = "~/.prism/buffer"

[unified_storage.s3]
bucket = "my-prism-bucket"
region = "us-east-1"
prefix = "collections/"
endpoint = "http://localhost:9000"  # for MinIO
force_path_style = true  # for MinIO
access_key_id = "..."  # optional, uses AWS chain
secret_access_key = "..."

[unified_storage.cache]
l1_path = "~/.prism/cache"
l1_max_size_gb = 10
write_through = true

# Embedding settings
[embedding]
enabled = true
model = "all-MiniLM-L6-v2"

# Logging settings
[logging]
level = "info"
file = "/var/log/prism.log"  # optional

# Observability settings
[observability]
log_format = "pretty"  # "pretty" or "json"
log_level = "info,prism=debug"
metrics_enabled = true

# Security settings
[security]
enabled = false

[[security.api_keys]]
key = "sk-abc123..."
name = "admin-key"
roles = ["admin"]

[[security.api_keys]]
key = "sk-reader..."
name = "readonly-key"
roles = ["reader"]

[security.roles.admin.collections]
"*" = ["read", "write", "delete", "admin"]

[security.roles.reader.collections]
"*" = ["read"]
"internal-*" = []  # deny

[security.audit]
enabled = true
index_to_collection = true
```

---

## 4. Collection Schema Reference

Collection schemas are YAML files in `<data_dir>/schemas/`.

### Full Schema Example

```yaml
collection: articles
description: "News articles with semantic search"

# Backend configuration
backends:
  # Text search (Tantivy)
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true
      - name: category
        type: string
        stored: true
        indexed: true
      - name: published_at
        type: date
        stored: true
        indexed: true
      - name: view_count
        type: i64
        stored: true
        indexed: true
    # BM25 tuning
    bm25_k1: 1.2    # term frequency saturation (default: 1.2)
    bm25_b: 0.75    # length normalization (default: 0.75)

  # Vector search (HNSW)
  vector:
    embedding_field: content_vector
    dimension: 384
    distance: cosine  # cosine, euclidean, dot
    hnsw_m: 16
    hnsw_ef_construction: 200
    hnsw_ef_search: 100
    vector_weight: 0.5

# Auto-generate embeddings
embedding_generation:
  enabled: true
  model: "all-MiniLM-L6-v2"
  source_field: content
  target_field: content_vector

# Indexing behavior
indexing:
  batch_size: 1000
  commit_interval_secs: 5
  worker_threads: 4

# Quotas
quota:
  max_documents: 1000000
  max_size_mb: 10240

# Faceted search
facets:
  allowed:
    - category
    - published_at
  default:
    - category
  configs:
    - field: category
      type: terms
      size: 10
    - field: published_at
      type: date_histogram
      interval: month

# Boosting/ranking
boosting:
  recency:
    field: published_at
    decay_function: exponential  # exponential, linear, gauss
    scale: "7d"
    offset: "1d"
    decay_rate: 0.5
  context:
    - field: project_id
      match_current: true
      boost: 2.0
  field_weights:
    title: 2.0
    content: 1.0
  signals:
    - name: view_count
      weight: 0.1

# System fields
system_fields:
  indexed_at: true       # auto _indexed_at timestamp
  document_boost: false  # per-doc _boost field

# Hybrid search defaults
hybrid:
  default_strategy: rrf  # rrf or weighted
  rrf_k: 60
  text_weight: 0.5
  vector_weight: 0.5

# Storage (per-collection override)
storage:
  backend: local
  data_dir: "./data/articles"
```

### Field Types

| Type | Description | Tantivy mapping |
|------|-------------|-----------------|
| `text` | Full-text searchable | TEXT with tokenizer |
| `string` | Exact match only | STRING |
| `i64` | Signed 64-bit integer | I64 |
| `u64` | Unsigned 64-bit integer | U64 |
| `f64` | 64-bit float | F64 |
| `bool` | Boolean | BOOL |
| `date` | Timestamp | DATE |
| `bytes` | Binary data | BYTES |

### Distance Metrics

| Metric | Description |
|--------|-------------|
| `cosine` | Cosine similarity (default, best for normalized vectors) |
| `euclidean` | L2 distance |
| `dot` | Dot product (for unnormalized vectors) |

---

## 5. Ingest Pipeline Configuration

Pipelines are YAML files in `conf/pipelines/`.

### Pipeline Definition

```yaml
name: normalize-content
description: "Normalize and clean content before indexing"

processors:
  - lowercase:
      field: title
  - html_strip:
      field: content
  - set:
      field: source
      value: "web-import"
  - remove:
      field: temp_field
  - rename:
      from: old_name
      to: new_name
```

### Available Processors

| Processor | Parameters | Description |
|-----------|------------|-------------|
| `lowercase` | `field` | Convert field to lowercase |
| `html_strip` | `field` | Remove HTML tags |
| `set` | `field`, `value` | Set field to fixed value |
| `remove` | `field` | Remove field |
| `rename` | `from`, `to` | Rename field |

---

## 6. API Endpoints Summary

See [API Reference](guides/api-reference.md) for full details.

### Core Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| POST | `/collections/:name/search` | Search collection |
| POST | `/collections/:name/documents` | Index documents |
| GET | `/collections/:name/documents/:id` | Get document |
| POST | `/collections/:name/aggregate` | Run aggregations |
| POST | `/collections/:name/_suggest` | Get suggestions |
| POST | `/collections/:name/_mlt` | More like this |
| POST | `/search/lucene` | Lucene DSL search |

### Admin Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/admin/collections` | List collections |
| GET | `/collections/:name/schema` | Get schema |
| GET | `/collections/:name/stats` | Get statistics |
| GET | `/admin/lint-schemas` | Validate schemas |

### Debug Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/collections/:name/terms/:field` | Top terms |
| GET | `/collections/:name/segments` | Segment info |
| GET | `/collections/:name/doc/:id/reconstruct` | Reconstruct doc |

### Metrics

| Method | Path | Description |
|--------|------|-------------|
| GET | `/metrics` | Prometheus metrics |

---

## 7. Prometheus Metrics

When `observability.metrics_enabled = true`:

### Search Metrics
```
prism_search_duration_seconds{collection, search_type}
prism_search_total{collection, search_type, status}
```

### Indexing Metrics
```
prism_index_duration_seconds{collection}
prism_index_documents_total{collection}
prism_index_batch_size{collection}
```

### HTTP Metrics
```
prism_http_requests_total{method, path, status}
prism_http_request_duration_seconds{method, path}
```

### Collection Metrics
```
prism_collections_count
```

### Embedding Metrics
```
prism_embedding_duration_seconds{provider}
prism_embedding_requests_total{provider, status}
prism_embedding_cache_hits_total{layer}
prism_embedding_cache_misses_total{layer}
```

---

## 8. Directory Structure

```
<data_dir>/
├── prism.toml              # Main config
├── conf/
│   ├── tls/
│   │   ├── cert.pem        # TLS certificate
│   │   └── key.pem         # TLS private key
│   └── pipelines/
│       └── *.yaml          # Ingest pipelines
├── schemas/
│   └── <collection>.yaml   # Collection schemas
├── data/
│   ├── text/               # Tantivy indexes
│   │   └── <collection>/
│   └── vector/             # HNSW indexes
│       └── <collection>/
├── cache/
│   └── models/             # Embedding model cache
└── logs/                   # Log files
```

---

## 9. Storage Backends

### Local Storage (Default)
```toml
[unified_storage]
backend = "local"
data_dir = "~/.prism/data"
```

### S3 Storage
```toml
[unified_storage]
backend = "s3"

[unified_storage.s3]
bucket = "my-bucket"
region = "us-east-1"
prefix = "prism/"
```

### MinIO (S3-Compatible)
```toml
[unified_storage]
backend = "s3"

[unified_storage.s3]
bucket = "local-bucket"
region = "us-east-1"
endpoint = "http://localhost:9000"
force_path_style = true
access_key_id = "minioadmin"
secret_access_key = "minioadmin"
```

### Cached Storage (L1 Local + L2 S3)
```toml
[unified_storage]
backend = "cached"

[unified_storage.s3]
bucket = "my-bucket"
region = "us-east-1"

[unified_storage.cache]
l1_path = "~/.prism/cache"
l1_max_size_gb = 10
write_through = true
```

---

## 10. Security Configuration

### API Key Authentication

```toml
[security]
enabled = true

[[security.api_keys]]
key = "sk-abc123def456"
name = "admin"
roles = ["admin"]

[[security.api_keys]]
key = "sk-reader789"
name = "readonly"
roles = ["reader"]
```

### Role-Based Access Control

```toml
[security.roles.admin.collections]
"*" = ["read", "write", "delete", "admin"]

[security.roles.reader.collections]
"*" = ["read"]
"private-*" = []  # explicit deny

[security.roles.writer.collections]
"public-*" = ["read", "write"]
```

### Audit Logging

```toml
[security.audit]
enabled = true
index_to_collection = true  # index audit logs to special collection
```

---

## 11. Docker Deployment

### docker-compose.yml

```yaml
version: '3.8'
services:
  prism:
    build: .
    ports:
      - "3080:3080"
    volumes:
      - ./data:/data
      - ./prism.toml:/etc/prism/prism.toml
    environment:
      - RUST_LOG=info,prism=debug
      - LOG_FORMAT=json
    command: ["prism-server", "-c", "/etc/prism/prism.toml", "--host", "0.0.0.0"]
```

### Dockerfile

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p prism-server

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/prism-server /usr/local/bin/
EXPOSE 3080
CMD ["prism-server"]
```

---

## 12. Quick Start

### 1. Create configuration
```bash
mkdir -p ~/.prism/schemas conf/pipelines
cat > prism.toml << 'EOF'
[storage]
data_dir = "~/.prism"

[observability]
log_format = "pretty"
log_level = "info,prism=debug"
metrics_enabled = true
EOF
```

### 2. Create collection schema
```bash
cat > ~/.prism/schemas/docs.yaml << 'EOF'
collection: docs
backends:
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true
  vector:
    embedding_field: embedding
    dimension: 384
embedding_generation:
  enabled: true
  model: all-MiniLM-L6-v2
  source_field: content
  target_field: embedding
EOF
```

### 3. Start server
```bash
prism-server -c prism.toml --port 3080
```

### 4. Index documents
```bash
curl -X POST http://localhost:3080/collections/docs/documents \
  -H "Content-Type: application/json" \
  -d '[{"id": "1", "title": "Hello", "content": "World"}]'
```

### 5. Search
```bash
curl -X POST http://localhost:3080/collections/docs/search \
  -H "Content-Type: application/json" \
  -d '{"query": "hello world", "limit": 10}'
```

---

*Generated: 2026-02-04*
