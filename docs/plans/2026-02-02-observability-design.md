# Observability Design: Metrics, Tracing, and Structured Logging

**Issue:** #38
**Scope:** Single-node foundation — Prometheus metrics, structured JSON logging, request tracing spans. Designed so clustering can extend it later.

---

## Architecture

Three capabilities built on Prism's existing `tracing` foundation:

1. **Prometheus metrics** via the `metrics` crate with `metrics-exporter-prometheus`. A `/metrics` endpoint on the main HTTP server exposes counters, histograms, and gauges in Prometheus text format. No separate port.

2. **Structured JSON logging** as an alternative to the current pretty-print format. Controlled by `[observability] log_format` in prism.toml (`"json"` or `"pretty"`, default `"pretty"`). `LOG_FORMAT` env var overrides the config value. Uses `tracing-subscriber`'s JSON layer.

3. **Request tracing spans** via `#[instrument]` on key functions. Spans appear in structured logs with timing and context fields. No external trace collector needed. OTLP export can be added later as a new tracing-subscriber layer without changing instrumentation code.

---

## Metrics

All exposed at `GET /metrics` in Prometheus text format.

### Query Metrics

- `prism_search_duration_seconds` (histogram) — labels: `collection`, `search_type` (text/vector/hybrid)
- `prism_search_total` (counter) — labels: `collection`, `search_type`, `status` (ok/error)
- `prism_search_results_count` (histogram) — labels: `collection`

### Indexing Metrics

- `prism_index_duration_seconds` (histogram) — labels: `collection`, `pipeline` (or "none")
- `prism_index_documents_total` (counter) — labels: `collection`, `status` (ok/error)
- `prism_index_batch_size` (histogram) — labels: `collection`

### Embedding Metrics

- `prism_embedding_duration_seconds` (histogram) — labels: `provider` (ort/ollama/openai)
- `prism_embedding_requests_total` (counter) — labels: `provider`, `status`
- `prism_embedding_cache_hits_total` (counter) — labels: `layer` (sqlite/redis)
- `prism_embedding_cache_misses_total` (counter) — labels: `layer`

### Server Metrics

- `prism_http_requests_total` (counter) — labels: `method`, `path`, `status_code`
- `prism_http_request_duration_seconds` (histogram) — labels: `method`, `path`
- `prism_collections_count` (gauge) — number of loaded collections

---

## Instrumentation (Tracing Spans)

`#[instrument]` added to key functions. Skip tight-loop internals to avoid overhead.

### API Layer (`prism/src/api/routes.rs`)

- `search()` — span with `collection`, `query` (truncated), `limit`
- `index_documents()` — span with `collection`, `pipeline`, `doc_count`
- `search_with_aggs()` — span with `collection`

### Backend Layer

- `TextBackend::search()` — span with `collection`, `query`
- `TextBackend::index()` — span with `collection`, `doc_count`
- `VectorBackend::search_text()` — span with `collection`
- `HybridSearchCoordinator::merge_results()` — span with `collection`

### Embedding Layer

- `VectorBackend::embed_text()` / `embed_texts()` — span with `provider`, `text_count`
- Cache lookup functions — span with `layer`

### Pipeline Layer

- `Pipeline::process()` — span with `pipeline_name`, `doc_id`

### HTTP Middleware

Enhance existing `TraceLayer` with `DefaultMakeSpan` and request ID generation. Every HTTP request gets a unique span; child spans nest under it.

---

## Configuration

New `[observability]` section in prism.toml:

```toml
[observability]
# Log format: "pretty" for development, "json" for production
# Override with LOG_FORMAT env var
log_format = "pretty"

# Log level filter (overridden by RUST_LOG env var)
log_level = "info,prism=debug"

# Enable Prometheus metrics at GET /metrics
metrics_enabled = true
```

Environment variable overrides:
- `LOG_FORMAT` overrides `log_format`
- `RUST_LOG` overrides `log_level`

---

## Dependencies

New crates:
- `metrics` — facade for recording metrics
- `metrics-exporter-prometheus` — Prometheus text format exporter

Existing crates (feature additions):
- `tracing-subscriber` — enable `json` feature

---

## Files Touched

- `Cargo.toml` — add workspace deps
- `prism/Cargo.toml` — add metrics deps
- `prism-server/Cargo.toml` — add metrics-exporter-prometheus
- `prism-server/src/main.rs` — init metrics recorder, configure log format from config
- `prism/src/config/mod.rs` — add `ObservabilityConfig` struct
- `prism/src/api/server.rs` — add `/metrics` route, enhance TraceLayer
- `prism/src/api/routes.rs` — add `#[instrument]` + metric recording to search/index handlers
- `prism/src/backends/text.rs` — `#[instrument]` on search/index
- `prism/src/backends/vector/backend.rs` — `#[instrument]` on search/embed
- `prism/src/backends/hybrid.rs` — `#[instrument]` on merge
- `prism/src/pipeline/registry.rs` — `#[instrument]` on process
- `prism/src/cache/sqlite.rs` — cache hit/miss counters
- `prism/src/embedding/model.rs` — embedding duration metrics

No new files — everything integrates into existing modules.

---

## Future Extensions

When clustering is added:
- Add OTLP trace export as a new tracing-subscriber layer (no instrumentation changes)
- Add cluster/shard/replication metrics
- Propagate trace context via inter-node RPC
- Add `metrics_port` config for dedicated metrics server
- Add `tracing_endpoint` and `tracing_sample_rate` config
