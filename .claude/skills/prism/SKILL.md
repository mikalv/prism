---
name: prism
description: Use when operating Prism hybrid search engine. Covers API usage, configuration, storage backends, encryption, backup, migration, monitoring, clustering, and troubleshooting production deployments.
---

# Prism Hybrid Search Engine

Production guide for operating Prism - a hybrid search engine combining full-text (BM25) and vector search (HNSW) for AI/RAG applications.

## Components

| Binary | Purpose |
|--------|---------|
| `prism-server` | HTTP API server (main process) |
| `prism-cli` | Collection management, backup, benchmarks |
| `prism-import` | Migrate from Elasticsearch |

---

## Starting & Stopping

```bash
# Start with config
prism-server -c /etc/prism/prism.toml

# Override bind address
prism-server -c prism.toml --host 0.0.0.0 -p 3080

# Debug mode
RUST_LOG=debug prism-server

# JSON logging (production)
LOG_FORMAT=json prism-server
```

**Signals:**
- `SIGTERM` / `SIGINT`: Graceful shutdown
- `SIGHUP`: Reload configuration (encryption keys, etc.)

**Health check:**
```bash
curl http://localhost:3080/health
# {"status": "ok"}
```

---

## Configuration (prism.toml)

### Minimal

```toml
[server]
bind_addr = "127.0.0.1:3080"

[storage]
data_dir = "/var/lib/prism"
```

### Production

```toml
[server]
bind_addr = "0.0.0.0:3080"

[server.tls]
enabled = true
bind_addr = "0.0.0.0:3443"
cert_path = "/etc/prism/cert.pem"
key_path = "/etc/prism/key.pem"

[server.cors]
enabled = true
origins = ["https://app.example.com"]

[storage]
data_dir = "/var/lib/prism"

[observability]
log_format = "json"
log_level = "info"
metrics_enabled = true

[security]
enabled = true

[[security.api_keys]]
key = "${PRISM_ADMIN_KEY}"
name = "admin"
roles = ["admin"]

[[security.api_keys]]
key = "${PRISM_APP_KEY}"
name = "app"
roles = ["reader"]

[security.roles.admin.collections]
"*" = ["read", "write", "delete", "admin"]

[security.roles.reader.collections]
"*" = ["read"]

[security.audit]
enabled = true
```

---

## Creating Collections

Schemas are YAML files in `<data_dir>/schemas/`. Server loads them on startup.

### Text-Only Collection

```yaml
# /var/lib/prism/schemas/articles.yaml
collection: articles
backends:
  text:
    fields:
      - name: id
        type: string
        stored: true
        indexed: true
      - name: title
        type: text
      - name: content
        type: text
      - name: category
        type: string
```

### Hybrid Collection (Text + Vector)

```yaml
# /var/lib/prism/schemas/knowledge.yaml
collection: knowledge
backends:
  text:
    fields:
      - name: id
        type: string
      - name: content
        type: text
  vector:
    embedding_field: content_vector
    dimension: 384
    distance: cosine

embedding_generation:
  enabled: true
  model: all-MiniLM-L6-v2
  source_field: content
  target_field: content_vector

hybrid:
  default_strategy: rrf
  rrf_k: 60
```

**After adding schema:** Restart server or wait for hot-reload.

**Verify:**
```bash
curl http://localhost:3080/admin/collections
curl http://localhost:3080/collections/knowledge/schema
```

---

## Indexing Documents

### Via API

```bash
curl -X POST http://localhost:3080/collections/articles/documents \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PRISM_ADMIN_KEY" \
  -d '[
    {"id": "1", "title": "First Article", "content": "..."},
    {"id": "2", "title": "Second Article", "content": "..."}
  ]'
```

### Via CLI (Bulk)

```bash
# JSONL format: one JSON object per line
prism-cli document import -c articles -f articles.jsonl --batch-size 500
```

---

## Searching

### Basic Search

```bash
curl -X POST http://localhost:3080/collections/articles/search \
  -H "Content-Type: application/json" \
  -d '{"query": "search terms", "limit": 10}'
```

### Hybrid Search (with Highlighting)

```bash
curl -X POST http://localhost:3080/collections/knowledge/search \
  -d '{
    "query": "machine learning concepts",
    "limit": 10,
    "merge_strategy": "rrf",
    "highlight": {
      "fields": ["content"],
      "pre_tag": "<em>",
      "post_tag": "</em>",
      "fragment_size": 150
    }
  }'
```

### Tune Hybrid Weights

```json
{
  "query": "specific keyword",
  "merge_strategy": "weighted",
  "text_weight": 0.7,
  "vector_weight": 0.3
}
```

| Use Case | text_weight | vector_weight |
|----------|-------------|---------------|
| Exact terms (code, IDs) | 0.7-0.8 | 0.2-0.3 |
| Balanced | 0.5 | 0.5 |
| Semantic (Q&A, chat) | 0.2-0.3 | 0.7-0.8 |

### Multi-Collection Search

```bash
# Wildcard patterns
curl -X POST "http://localhost:3080/logs-*/_search" -d '{"query": "error"}'

# Comma-separated
curl -X POST "http://localhost:3080/logs-2026-01,logs-2026-02/_search" -d '{"query": "error"}'
```

### Aggregations

```bash
curl -X POST http://localhost:3080/collections/products/aggregate \
  -d '{
    "query": "*",
    "aggregations": [
      {"name": "total", "type": "count"},
      {"name": "by_category", "type": "terms", "field": "category", "size": 10},
      {"name": "price_stats", "type": "stats", "field": "price"},
      {"name": "price_histogram", "type": "histogram", "field": "price", "interval": 50}
    ]
  }'
```

---

## Storage Backends

### Backend Types

| Backend | When to Use |
|---------|-------------|
| `local` | Single node, development (default) |
| `s3` | Cloud storage, durability, scaling |
| `cached` | L1 local + L2 S3 (best for production) |
| `compressed` | Reduce storage (LZ4 fast, Zstd balanced) |
| `encrypted` | Compliance, multi-tenant, sensitive data |

### Local (Default)

```toml
[storage]
backend = "local"
data_dir = "/var/lib/prism"
```

### S3

```toml
[unified_storage]
backend = "s3"

[unified_storage.s3]
bucket = "my-prism-bucket"
region = "us-east-1"
```

Credentials: AWS environment variables, IAM role, or explicit config.

### Cached (L1 Local + L2 S3)

```toml
[unified_storage]
backend = "cached"

[unified_storage.s3]
bucket = "my-bucket"
region = "us-east-1"

[unified_storage.cache]
l1_path = "/var/lib/prism/cache"
l1_max_size_gb = 50
write_through = true
```

### Compressed

```toml
[storage]
backend = "compressed"

[storage.compressed]
algorithm = "zstd"  # or "lz4", "zstd:9"
min_size = 1024

[storage.compressed.inner]
backend = "local"
data_dir = "/var/lib/prism"
```

### Encrypted

```toml
[storage]
backend = "encrypted"

[storage.encrypted]
key_source = "env"
key_env_var = "PRISM_ENCRYPTION_KEY"

[storage.encrypted.inner]
backend = "local"
data_dir = "/var/lib/prism"
```

Set key: `export PRISM_ENCRYPTION_KEY="<64 hex chars>"`

### Layered (Encrypted + Compressed + S3)

```toml
[storage]
backend = "encrypted"

[storage.encrypted]
key_source = "env"
key_env_var = "PRISM_ENCRYPTION_KEY"

[storage.encrypted.inner]
backend = "compressed"
algorithm = "zstd"

[storage.encrypted.inner.inner]
backend = "s3"

[storage.encrypted.inner.inner.s3]
bucket = "secure-bucket"
region = "us-east-1"
```

---

## Encryption Operations

### Generate Key

```bash
curl -X POST http://localhost:3080/_admin/encryption/generate-key
# {"key": "a1b2c3d4...64 hex chars", "algorithm": "AES-256-GCM"}
```

**Save this key securely!** Prism never stores keys.

### Encrypted Export (Runtime Key)

Export collection with key provided at runtime (no restart needed):

```bash
curl -X POST http://localhost:3080/_admin/export/encrypted \
  -H "Content-Type: application/json" \
  -d '{
    "collection": "sensitive-data",
    "key": "<64 hex chars>",
    "output_path": "/backup/sensitive.enc"
  }'
```

### Encrypted Import

```bash
curl -X POST http://localhost:3080/_admin/import/encrypted \
  -H "Content-Type: application/json" \
  -d '{
    "input_path": "/backup/sensitive.enc",
    "key": "<64 hex chars>",
    "target_collection": "sensitive-restored"
  }'
```

---

## Backup & Restore

### Export Formats

| Format | File | Use Case |
|--------|------|----------|
| Portable | `.prism.jsonl` | Cross-version, human-readable |
| Snapshot | `.tar.zst` | Fast, same-version only |
| Encrypted | `.enc` | Secure cloud storage |

### CLI Backup

```bash
# Portable (cross-version safe)
prism-cli collection export articles -o articles.prism.jsonl

# Snapshot (fast, same-version)
prism-cli collection export articles -o articles.tar.zst --format snapshot

# Restore
prism-cli collection restore -f articles.prism.jsonl
prism-cli collection restore -f articles.tar.zst --format snapshot
```

### Encrypted Cloud Backup Script

```bash
#!/bin/bash
KEY_FILE="/secure/prism-key.txt"
BUCKET="s3://my-backups/prism"

# Generate or read key
if [ ! -f "$KEY_FILE" ]; then
  curl -s -X POST http://localhost:3080/_admin/encryption/generate-key | jq -r .key > "$KEY_FILE"
  chmod 600 "$KEY_FILE"
fi

KEY=$(cat "$KEY_FILE")
DATE=$(date +%Y-%m-%d)

for collection in $(curl -s http://localhost:3080/admin/collections | jq -r '.[]'); do
  curl -s -X POST http://localhost:3080/_admin/export/encrypted \
    -d "{\"collection\": \"$collection\", \"key\": \"$KEY\", \"output_path\": \"/tmp/$collection.enc\"}"
  aws s3 cp "/tmp/$collection.enc" "$BUCKET/$DATE/$collection.enc"
  rm "/tmp/$collection.enc"
done
```

### Disaster Recovery (Disk Full)

When disk unexpectedly fills:

1. **Generate key:**
   ```bash
   curl -X POST http://localhost:3080/_admin/encryption/generate-key > /secure/emergency-key.json
   ```

2. **Export to external storage:**
   ```bash
   KEY=$(jq -r .key /secure/emergency-key.json)
   for c in logs metrics events; do
     curl -X POST http://localhost:3080/_admin/export/encrypted \
       -d "{\"collection\": \"$c\", \"key\": \"$KEY\", \"output_path\": \"/mnt/external/$c.enc\"}"
   done
   ```

3. **Delete local to free space:**
   ```bash
   prism-cli collection delete logs metrics events
   ```

4. **Restore when ready:**
   ```bash
   for c in logs metrics events; do
     curl -X POST http://localhost:3080/_admin/import/encrypted \
       -d "{\"input_path\": \"/mnt/external/$c.enc\", \"key\": \"$KEY\"}"
   done
   ```

---

## Migration

### Between Storage Backends (Local → S3)

```bash
# 1. Export all collections
for c in $(curl -s http://localhost:3080/admin/collections | jq -r '.[]'); do
  prism-cli document export -c "$c" -o "backup-$c.jsonl"
done

# 2. Stop server, update prism.toml to S3 backend

# 3. Start server, re-import
for f in backup-*.jsonl; do
  c=$(echo "$f" | sed 's/backup-\(.*\)\.jsonl/\1/')
  prism-cli document import -c "$c" -f "$f"
done
```

### Between Prism Versions

Use portable format for cross-version compatibility:

```bash
# Old version
prism-cli collection export myindex -o myindex.prism.jsonl

# New version
prism-cli collection restore -f myindex.prism.jsonl
```

### From Elasticsearch

```bash
prism-import --source http://elasticsearch:9200 --index logs-* --target http://localhost:3080
```

---

## Monitoring

### Enable Metrics

```toml
[observability]
metrics_enabled = true
log_format = "json"
log_level = "info"
```

### Prometheus Scrape

```yaml
scrape_configs:
  - job_name: 'prism'
    static_configs:
      - targets: ['prism:3080']
    metrics_path: /metrics
    scrape_interval: 15s
```

### Key Metrics

| Metric | What to Watch |
|--------|---------------|
| `prism_search_duration_seconds` | p99 latency spikes |
| `prism_search_total{status="error"}` | Error rate |
| `prism_embedding_cache_hits_total` | Low hit rate = high embedding cost |
| `prism_index_documents_total` | Indexing throughput |

### Grafana Queries

```promql
# Search p99 latency
histogram_quantile(0.99, sum(rate(prism_search_duration_seconds_bucket[5m])) by (le))

# Error rate percentage
100 * sum(rate(prism_search_total{status="error"}[5m])) / sum(rate(prism_search_total[5m]))

# Cache hit ratio
sum(rate(prism_embedding_cache_hits_total[5m])) /
(sum(rate(prism_embedding_cache_hits_total[5m])) + sum(rate(prism_embedding_cache_misses_total[5m])))
```

### Alerting Rules

```yaml
groups:
  - name: prism
    rules:
      - alert: PrismHighLatency
        expr: histogram_quantile(0.99, sum(rate(prism_search_duration_seconds_bucket[5m])) by (le)) > 1
        for: 5m
        labels:
          severity: warning

      - alert: PrismHighErrorRate
        expr: sum(rate(prism_search_total{status="error"}[5m])) / sum(rate(prism_search_total[5m])) > 0.05
        for: 5m
        labels:
          severity: critical

      - alert: PrismDown
        expr: up{job="prism"} == 0
        for: 1m
        labels:
          severity: critical
```

---

## Clustering

### Deployment Modes

| Mode | Use Case |
|------|----------|
| Single node | Development, small datasets |
| Federation | Read scaling, simple HA |
| Cluster | Full distributed, auto-failover |

### Node Discovery

```toml
[federation.discovery]
backend = "static"  # or "dns", "gossip"
nodes = ["node1:3000", "node2:3000"]

# For Kubernetes
backend = "dns"
dns_name = "prism-headless.default.svc.cluster.local"
```

### Inter-Node Communication

```toml
[cluster.transport]
transport = "quic"
bind_port = 7000
cert_path = "/etc/prism/cluster-cert.pem"
key_path = "/etc/prism/cluster-key.pem"
```

### Replication

```toml
[collection.replication]
factor = 2
min_replicas_for_write = 1

# Zone-aware placement
[node]
id = "node-1"
zone = "eu-west-1a"
```

---

## Index Maintenance

### Optimize (Merge Segments)

```bash
prism-cli index optimize -c articles
```

**When to run:**
- After bulk imports
- After many deletes
- Weekly/monthly maintenance
- When search performance degrades

### Cache Management

```bash
# View stats
prism-cli cache-stats -p /var/lib/prism/cache

# Clear old entries
prism-cli cache-clear -p /var/lib/prism/cache --older-than-days 30

# Clear all
prism-cli cache-clear -p /var/lib/prism/cache
```

---

## Troubleshooting

### Server Won't Start

```bash
# Check config syntax
prism-server -c prism.toml --check

# Verbose logging
RUST_LOG=debug prism-server -c prism.toml
```

**Common causes:**
- Port already in use
- Invalid TOML syntax
- Schema validation errors
- Missing directories

### Slow Searches

1. **Check segment count:**
   ```bash
   prism-cli collection inspect -n articles -v
   ```
   Many segments = run optimize

2. **Check cache hit rate:**
   ```bash
   curl http://localhost:3080/stats/cache
   ```
   Low hit rate = cold cache or missing cache config

3. **Benchmark:**
   ```bash
   prism-cli benchmark -c articles -q queries.txt -r 50
   ```

### Decryption Failed

- Wrong key (keys must match exactly)
- Truncated/corrupted file
- Key must be 64 hex characters (256 bits)

### Schema Conflicts on Import

```bash
# Delete conflicting collection first
prism-cli collection delete myindex

# Then restore
prism-cli collection restore -f backup.prism.jsonl
```

### Out of Memory

- Reduce batch sizes for imports
- Enable compression backend
- Increase resource limits
- Check vector dimension (384d = ~2KB/doc, 1536d = ~8KB/doc)

---

## API Quick Reference

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/metrics` | GET | Prometheus metrics |
| `/admin/collections` | GET | List collections |
| `/collections/:c/schema` | GET | Get schema |
| `/collections/:c/stats` | GET | Collection stats |
| `/collections/:c/search` | POST | Search |
| `/collections/:c/documents` | POST | Index documents |
| `/collections/:c/documents/:id` | GET | Get document |
| `/collections/:c/aggregate` | POST | Aggregations |
| `/collections/:c/_suggest` | POST | Autocomplete |
| `/collections/:c/_mlt` | POST | More like this |
| `/:collections/_search` | POST | Multi-collection search |
| `/_admin/export/encrypted` | POST | Encrypted export |
| `/_admin/import/encrypted` | POST | Encrypted import |
| `/_admin/encryption/generate-key` | POST | Generate AES key |

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level (`info`, `debug`, `warn`, `prism=debug`) |
| `LOG_FORMAT` | `pretty` or `json` |
| `PRISM_ENCRYPTION_KEY` | AES-256 key (64 hex chars) |
| `PRISM_ADMIN_KEY` | Admin API key |
| `AWS_ACCESS_KEY_ID` | S3 credentials |
| `AWS_SECRET_ACCESS_KEY` | S3 credentials |
| `AWS_REGION` | S3 region |
