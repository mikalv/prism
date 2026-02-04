# Monitoring

Prism exposes Prometheus metrics and structured logging for observability.

## Enabling Metrics

```toml
[observability]
metrics_enabled = true
```

Metrics are available at `/metrics`:

```bash
curl http://localhost:3080/metrics
```

## Available Metrics

### Search Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `prism_search_duration_seconds` | Histogram | `collection`, `search_type` | Search latency |
| `prism_search_total` | Counter | `collection`, `search_type`, `status` | Total searches |

Search types: `text`, `vector`, `hybrid`

### Indexing Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `prism_index_duration_seconds` | Histogram | `collection` | Index operation latency |
| `prism_index_documents_total` | Counter | `collection` | Total documents indexed |
| `prism_index_batch_size` | Histogram | `collection` | Documents per batch |

### HTTP Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `prism_http_requests_total` | Counter | `method`, `path`, `status` | Total HTTP requests |
| `prism_http_request_duration_seconds` | Histogram | `method`, `path` | Request latency |

### Collection Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `prism_collections_count` | Gauge | — | Number of collections |

### Embedding Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `prism_embedding_duration_seconds` | Histogram | `provider` | Embedding generation latency |
| `prism_embedding_requests_total` | Counter | `provider`, `status` | Total embedding requests |
| `prism_embedding_cache_hits_total` | Counter | `layer` | Cache hits |
| `prism_embedding_cache_misses_total` | Counter | `layer` | Cache misses |

## Prometheus Configuration

Add Prism to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'prism'
    static_configs:
      - targets: ['localhost:3080']
    metrics_path: /metrics
    scrape_interval: 15s
```

## Grafana Dashboard

Example queries for a Grafana dashboard:

### Search Latency (p99)

```promql
histogram_quantile(0.99,
  sum(rate(prism_search_duration_seconds_bucket[5m])) by (le, collection)
)
```

### Search Throughput

```promql
sum(rate(prism_search_total[5m])) by (collection, search_type)
```

### Error Rate

```promql
sum(rate(prism_search_total{status="error"}[5m]))
/
sum(rate(prism_search_total[5m]))
```

### Indexing Rate

```promql
sum(rate(prism_index_documents_total[5m])) by (collection)
```

### Cache Hit Ratio

```promql
sum(rate(prism_embedding_cache_hits_total[5m]))
/
(sum(rate(prism_embedding_cache_hits_total[5m])) + sum(rate(prism_embedding_cache_misses_total[5m])))
```

## Logging

### Configuration

```toml
[observability]
log_format = "pretty"    # or "json"
log_level = "info,prism=debug"
```

### Log Levels

| Level | Use |
|-------|-----|
| `error` | Errors only |
| `warn` | Warnings and errors |
| `info` | Normal operations (default) |
| `debug` | Detailed debugging |
| `trace` | Very verbose |

### Per-Module Logging

Use Rust-style log filters:

```toml
log_level = "info,prism=debug,prism::search=trace"
```

Or via environment:

```bash
RUST_LOG="info,prism=debug" prism-server
```

### JSON Logging

For log aggregation (ELK, Loki, etc.):

```toml
[observability]
log_format = "json"
```

Output:

```json
{"timestamp":"2025-01-15T10:30:00Z","level":"INFO","target":"prism::search","message":"Search completed","collection":"docs","duration_ms":45}
```

## Health Checks

### Liveness

```bash
curl http://localhost:3080/health
```

Returns `200 OK` if server is running.

### Readiness

```bash
curl http://localhost:3080/health
```

Suitable for Kubernetes probes:

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 3080
  initialDelaySeconds: 5
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /health
    port: 3080
  initialDelaySeconds: 5
  periodSeconds: 10
```

## Alerting Rules

Example Prometheus alerting rules:

```yaml
groups:
  - name: prism
    rules:
      - alert: PrismHighLatency
        expr: histogram_quantile(0.99, sum(rate(prism_search_duration_seconds_bucket[5m])) by (le)) > 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Prism search latency is high"
          description: "p99 latency is {{ $value }}s"

      - alert: PrismHighErrorRate
        expr: sum(rate(prism_search_total{status="error"}[5m])) / sum(rate(prism_search_total[5m])) > 0.05
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "Prism error rate is high"
          description: "Error rate is {{ $value | humanizePercentage }}"

      - alert: PrismDown
        expr: up{job="prism"} == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Prism is down"
```

## See Also

- [Configuration](configuration.md) — Full config reference
- [Deployment](deployment.md) — Production setup
