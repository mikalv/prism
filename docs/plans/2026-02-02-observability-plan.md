# Observability Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Prometheus metrics, structured JSON logging, and request tracing spans to single-node Prism.

**Architecture:** Build on existing `tracing` foundation. Add `metrics` crate for Prometheus export via `/metrics` route. Add JSON log layer controlled by config. Add `#[instrument]` spans to search, indexing, embedding, and pipeline hot paths.

**Tech Stack:** metrics, metrics-exporter-prometheus, tracing, tracing-subscriber (json feature), axum

---

### Task 1: Add dependencies to workspace

**Files:**
- Modify: `Cargo.toml` (workspace deps, lines 32-34)
- Modify: `prism/Cargo.toml` (add metrics dep, lines 43-45)
- Modify: `prism-server/Cargo.toml` (add metrics-exporter-prometheus)

**Step 1: Add workspace dependencies**

In `Cargo.toml`, add after the existing `tracing-subscriber` line (line 34):

```toml
metrics = "0.24"
metrics-exporter-prometheus = "0.16"
```

And change the tracing-subscriber line to add the `json` feature:

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
```

**Step 2: Add prism crate dependency**

In `prism/Cargo.toml`, add after the `tracing-subscriber` line (line 45):

```toml
metrics = { workspace = true }
```

**Step 3: Add prism-server dependency**

In `prism-server/Cargo.toml`, add:

```toml
metrics = { workspace = true }
metrics-exporter-prometheus = { workspace = true }
```

**Step 4: Verify it compiles**

Run: `cargo check -p prism -p prism-server 2>&1 | tail -5`
Expected: Compiles with no errors

**Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock prism/Cargo.toml prism-server/Cargo.toml
git commit -m "feat(observability): add metrics and prometheus dependencies"
```

---

### Task 2: Add ObservabilityConfig to config module

**Files:**
- Modify: `prism/src/config/mod.rs`

**Step 1: Write the test**

Create a test in `prism/tests/config_test.rs` (append to existing file):

```rust
#[test]
fn test_observability_config_defaults() {
    let config = Config::default();
    assert_eq!(config.observability.log_format, "pretty");
    assert_eq!(config.observability.log_level, "info,prism=debug");
    assert!(config.observability.metrics_enabled);
}

#[test]
fn test_observability_config_from_toml() {
    let toml_str = r#"
[observability]
log_format = "json"
log_level = "debug"
metrics_enabled = false
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.observability.log_format, "json");
    assert_eq!(config.observability.log_level, "debug");
    assert!(!config.observability.metrics_enabled);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p prism test_observability_config 2>&1 | tail -10`
Expected: FAIL — `observability` field doesn't exist on Config

**Step 3: Add ObservabilityConfig struct**

In `prism/src/config/mod.rs`, add after `LoggingConfig` (after line 248):

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObservabilityConfig {
    /// Log output format: "pretty" or "json"
    /// Override with LOG_FORMAT env var
    #[serde(default = "default_log_format")]
    pub log_format: String,

    /// Log level filter string
    /// Override with RUST_LOG env var
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Enable Prometheus metrics at GET /metrics
    #[serde(default = "default_true")]
    pub metrics_enabled: bool,
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_log_level() -> String {
    "info,prism=debug".to_string()
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            log_format: default_log_format(),
            log_level: default_log_level(),
            metrics_enabled: true,
        }
    }
}
```

**Step 4: Add field to Config struct**

In the `Config` struct (line 16-32), add:

```rust
    #[serde(default)]
    pub observability: ObservabilityConfig,
```

And in `impl Default for Config` (line 250-261), add:

```rust
            observability: ObservabilityConfig::default(),
```

**Step 5: Run tests**

Run: `cargo test -p prism test_observability_config 2>&1 | tail -10`
Expected: PASS

**Step 6: Commit**

```bash
git add prism/src/config/mod.rs prism/tests/config_test.rs
git commit -m "feat(observability): add ObservabilityConfig to config module"
```

---

### Task 3: Set up metrics recorder and structured logging in prism-server

**Files:**
- Modify: `prism-server/src/main.rs`

**Step 1: Update tracing initialization to use config**

Replace the tracing init block (lines 25-31) and restructure main.rs so config is loaded before tracing init. The new `main()` should be:

```rust
use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "prism-server")]
#[command(about = "Prism hybrid search server")]
#[command(version)]
struct Args {
    /// Configuration file path
    #[arg(short, long, default_value = "prism.toml")]
    config: String,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on
    #[arg(short, long, default_value = "3080")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Load config first (tracing init depends on it)
    let config = prism::config::Config::load_or_create(std::path::Path::new(&args.config))?;

    // Determine log format: env var overrides config
    let log_format = std::env::var("LOG_FORMAT")
        .unwrap_or_else(|_| config.observability.log_format.clone());

    // Determine log level: RUST_LOG overrides config
    let log_level = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| config.observability.log_level.clone());

    // Initialize tracing with configured format
    let env_filter = tracing_subscriber::EnvFilter::new(&log_level);
    let registry = tracing_subscriber::registry().with(env_filter);

    if log_format == "json" {
        registry
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        registry
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    // Initialize Prometheus metrics recorder
    if config.observability.metrics_enabled {
        let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
        // Install as global recorder — metrics! macros work everywhere after this
        builder
            .install()
            .expect("Failed to install Prometheus metrics recorder");
        tracing::info!("Prometheus metrics enabled at /metrics");
    }

    config.ensure_dirs()?;

    // ... rest of main unchanged from line 35 onward ...
```

Note: `PrometheusBuilder::install()` installs a global recorder but doesn't serve an HTTP endpoint — we'll add the `/metrics` route in Task 4.

Wait — `install()` starts its own HTTP server. We want to serve on the same port. Use `install_recorder()` instead, which returns a `PrometheusHandle` we can use in the axum route.

Corrected approach:

```rust
    // Initialize Prometheus metrics recorder
    let metrics_handle = if config.observability.metrics_enabled {
        let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
        let handle = builder
            .install_recorder()
            .expect("Failed to install Prometheus metrics recorder");
        tracing::info!("Prometheus metrics enabled at /metrics");
        Some(handle)
    } else {
        None
    };
```

Then pass `metrics_handle` to `ApiServer` (we'll update that in Task 4).

**Step 2: Pass metrics handle to ApiServer**

After the server creation (line 69-74), change to:

```rust
    let server = prism::api::ApiServer::with_pipelines(
        manager,
        config.server.cors.clone(),
        config.security.clone(),
        pipeline_registry,
    )
    .with_metrics(metrics_handle);
```

**Step 3: Verify it compiles (it won't yet — `with_metrics` doesn't exist)**

Run: `cargo check -p prism-server 2>&1 | tail -5`
Expected: FAIL — `with_metrics` method not found. That's expected, Task 4 adds it.

**Step 4: Commit the main.rs changes**

```bash
git add prism-server/src/main.rs
git commit -m "feat(observability): configure structured logging and metrics recorder"
```

---

### Task 4: Add /metrics route and metrics handle to ApiServer

**Files:**
- Modify: `prism/src/api/server.rs`

**Step 1: Add metrics handle to AppState and ApiServer**

In `server.rs`, add to the `AppState` struct:

```rust
pub metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
```

Add to `ApiServer`:

```rust
metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
```

Add a builder method:

```rust
pub fn with_metrics(mut self, handle: Option<metrics_exporter_prometheus::PrometheusHandle>) -> Self {
    self.metrics_handle = handle;
    self
}
```

Initialize `metrics_handle: None` in the existing constructors.

**Step 2: Add /metrics route**

In the `router()` method, add a `/metrics` route before the middleware layers. The handler reads from the PrometheusHandle:

```rust
async fn metrics_handler(
    State(state): State<AppState>,
) -> impl axum::response::IntoResponse {
    match &state.metrics_handle {
        Some(handle) => {
            let metrics = handle.render();
            (
                StatusCode::OK,
                [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                metrics,
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            [("content-type", "text/plain; charset=utf-8")],
            "Metrics not enabled".to_string(),
        ),
    }
}
```

Add the route to the pipeline_routes section (which uses AppState):

```rust
.route("/metrics", get(metrics_handler))
```

**Step 3: Verify it compiles**

Run: `cargo check -p prism -p prism-server 2>&1 | tail -5`
Expected: Compiles

**Step 4: Run all tests**

Run: `cargo test -p prism 2>&1 | tail -10`
Expected: All existing tests pass

**Step 5: Commit**

```bash
git add prism/src/api/server.rs
git commit -m "feat(observability): add /metrics endpoint for Prometheus"
```

---

### Task 5: Add HTTP request metrics middleware

**Files:**
- Modify: `prism/src/api/server.rs`

**Step 1: Create metrics middleware**

Add an axum middleware function that records `prism_http_requests_total` and `prism_http_request_duration_seconds` for every request:

```rust
use std::time::Instant;

async fn metrics_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    let response = next.run(req).await;

    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    metrics::counter!("prism_http_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status_code" => status,
    )
    .increment(1);

    metrics::histogram!("prism_http_request_duration_seconds",
        "method" => method,
        "path" => path,
    )
    .record(duration);

    response
}
```

**Step 2: Add middleware to router**

In the `router()` method, add after the TraceLayer (line 300):

```rust
.layer(axum::middleware::from_fn(metrics_middleware))
```

**Step 3: Verify it compiles**

Run: `cargo check -p prism 2>&1 | tail -5`
Expected: Compiles

**Step 4: Commit**

```bash
git add prism/src/api/server.rs
git commit -m "feat(observability): add HTTP request metrics middleware"
```

---

### Task 6: Instrument search path with metrics and spans

**Files:**
- Modify: `prism/src/api/routes.rs` (search handler)
- Modify: `prism/src/backends/text.rs` (TextBackend::search)
- Modify: `prism/src/backends/hybrid.rs` (merge_results)

**Step 1: Add #[instrument] and metrics to search handler**

In `prism/src/api/routes.rs`, add `use metrics;` at the top if not present, and modify the `search` function (line 85):

```rust
#[tracing::instrument(
    name = "search",
    skip(manager, request),
    fields(collection = %collection, search_type = "text")
)]
pub async fn search(
    Path(collection): Path<String>,
    State(manager): State<Arc<CollectionManager>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResults>, StatusCode> {
    let start = std::time::Instant::now();

    // ... existing body ...

    // Before the return, record metrics:
    // On success path:
    let duration = start.elapsed().as_secs_f64();
    metrics::histogram!("prism_search_duration_seconds",
        "collection" => collection.clone(),
        "search_type" => "text",
    ).record(duration);
    metrics::counter!("prism_search_total",
        "collection" => collection.clone(),
        "search_type" => "text",
        "status" => "ok",
    ).increment(1);
```

Wrap the error path similarly with `"status" => "error"`.

**Step 2: Add #[instrument] to TextBackend::search**

In `prism/src/backends/text.rs`, add to the `search` method:

```rust
#[tracing::instrument(name = "text_search", skip(self), fields(collection = %collection))]
```

**Step 3: Add #[instrument] to HybridSearchCoordinator::merge_results**

In `prism/src/backends/hybrid.rs`:

```rust
#[tracing::instrument(name = "merge_results", skip(self, text_results, vector_results))]
```

**Step 4: Verify it compiles**

Run: `cargo check -p prism 2>&1 | tail -5`
Expected: Compiles

**Step 5: Run tests**

Run: `cargo test -p prism 2>&1 | tail -10`
Expected: All pass

**Step 6: Commit**

```bash
git add prism/src/api/routes.rs prism/src/backends/text.rs prism/src/backends/hybrid.rs
git commit -m "feat(observability): instrument search path with metrics and spans"
```

---

### Task 7: Instrument indexing path with metrics and spans

**Files:**
- Modify: `prism/src/api/routes.rs` (index_documents handler)
- Modify: `prism/src/backends/text.rs` (TextBackend::index)

**Step 1: Add #[instrument] and metrics to index_documents**

In `prism/src/api/routes.rs`, modify `index_documents` (line 181):

```rust
#[tracing::instrument(
    name = "index_documents",
    skip(state, request),
    fields(
        collection = %collection,
        pipeline = query.pipeline.as_deref().unwrap_or("none"),
    )
)]
pub async fn index_documents(
```

Add timing and metrics recording:

```rust
    let start = std::time::Instant::now();
    // ... existing body ...

    let duration = start.elapsed().as_secs_f64();
    let pipeline_label = query.pipeline.as_deref().unwrap_or("none").to_string();

    metrics::histogram!("prism_index_duration_seconds",
        "collection" => collection.clone(),
        "pipeline" => pipeline_label.clone(),
    ).record(duration);

    metrics::counter!("prism_index_documents_total",
        "collection" => collection.clone(),
        "status" => "ok",
    ).increment(indexed as u64);

    metrics::histogram!("prism_index_batch_size",
        "collection" => collection.clone(),
    ).record(total as f64);
```

**Step 2: Add #[instrument] to TextBackend::index**

```rust
#[tracing::instrument(name = "text_index", skip(self, documents), fields(collection = %collection, doc_count = documents.len()))]
```

**Step 3: Verify and test**

Run: `cargo check -p prism && cargo test -p prism 2>&1 | tail -10`
Expected: Compiles and all tests pass

**Step 4: Commit**

```bash
git add prism/src/api/routes.rs prism/src/backends/text.rs
git commit -m "feat(observability): instrument indexing path with metrics and spans"
```

---

### Task 8: Instrument embedding and cache paths

**Files:**
- Modify: `prism/src/backends/vector/backend.rs`
- Modify: `prism/src/cache/sqlite.rs`

**Step 1: Add #[instrument] and metrics to embedding functions**

In `prism/src/backends/vector/backend.rs`:

```rust
#[tracing::instrument(name = "embed_text", skip(self, text))]
pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
    let start = std::time::Instant::now();
    // ... existing body ...
    // Before return:
    metrics::histogram!("prism_embedding_duration_seconds",
        "provider" => "ort",  // or detect from config
    ).record(start.elapsed().as_secs_f64());
    metrics::counter!("prism_embedding_requests_total",
        "provider" => "ort",
        "status" => "ok",
    ).increment(1);
```

Add same to `embed_texts` and `search_text`:

```rust
#[tracing::instrument(name = "embed_texts", skip(self, texts), fields(text_count = texts.len()))]
#[tracing::instrument(name = "vector_search", skip(self), fields(collection = %collection))]
```

**Step 2: Add cache hit/miss counters**

In `prism/src/cache/sqlite.rs`, in the `get` method:

```rust
// On cache hit (Some result):
metrics::counter!("prism_embedding_cache_hits_total", "layer" => "sqlite").increment(1);

// On cache miss (None):
metrics::counter!("prism_embedding_cache_misses_total", "layer" => "sqlite").increment(1);
```

**Step 3: Verify and test**

Run: `cargo check -p prism && cargo test -p prism 2>&1 | tail -10`
Expected: Compiles and all tests pass

**Step 4: Commit**

```bash
git add prism/src/backends/vector/backend.rs prism/src/cache/sqlite.rs
git commit -m "feat(observability): instrument embedding and cache paths"
```

---

### Task 9: Instrument pipeline processing

**Files:**
- Modify: `prism/src/pipeline/registry.rs`

**Step 1: Add #[instrument] to Pipeline::process**

```rust
#[tracing::instrument(name = "pipeline_process", skip(self, doc), fields(pipeline = %self.name))]
pub fn process(&self, doc: &mut Document) -> Result<()> {
```

**Step 2: Verify and test**

Run: `cargo test -p prism pipeline 2>&1 | tail -10`
Expected: All pipeline tests pass

**Step 3: Commit**

```bash
git add prism/src/pipeline/registry.rs
git commit -m "feat(observability): instrument pipeline processing"
```

---

### Task 10: Add collections count gauge

**Files:**
- Modify: `prism/src/collection/manager.rs`

**Step 1: Add gauge update after collection loading**

In the `initialize()` method of `CollectionManager`, after collections are loaded:

```rust
metrics::gauge!("prism_collections_count").set(collections.len() as f64);
```

Also update after any collection creation/deletion if those methods exist.

**Step 2: Verify and test**

Run: `cargo check -p prism && cargo test -p prism 2>&1 | tail -10`
Expected: Compiles and all tests pass

**Step 3: Commit**

```bash
git add prism/src/collection/manager.rs
git commit -m "feat(observability): add collections count gauge"
```

---

### Task 11: Final verification and format

**Step 1: Run cargo fmt**

Run: `cargo fmt --all`

**Step 2: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20`
Expected: No warnings

**Step 3: Run full test suite**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: All tests pass

**Step 4: Verify metrics output manually**

Start the server and check metrics:

```bash
cargo run -p prism-server -- --config /dev/null &
sleep 2
curl -s http://localhost:3080/metrics | head -20
kill %1
```

Expected: Prometheus text format output with `prism_` prefixed metrics.

**Step 5: Test JSON logging**

```bash
LOG_FORMAT=json cargo run -p prism-server -- --config /dev/null 2>&1 | head -5
```

Expected: JSON-formatted log lines.

**Step 6: Commit any fixes**

```bash
git add -A
git commit -m "fix(observability): address issues found during verification"
```
