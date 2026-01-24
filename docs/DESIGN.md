# Prism Extraction Plan

**Date:** 2026-01-24
**Status:** Draft
**Author:** mikalv

## Overview

Extract `platform/searchcore` from llmcoder into a standalone product called **Prism** - a hybrid search engine combining full-text and vector search for AI/RAG applications.

## Goals

1. Phase out searchcore from llmcoder into its own repository
2. Make Prism a standalone, publishable product
3. Add vector/embedding cache (query Ollama only once per keyword)
4. Comprehensive documentation
5. Docker image with minimal footprint

## Target Audience

- **AI/RAG developers** - need hybrid search (text + vector) for LLM context
- **Enterprise users** - replacing Elasticsearch/Weaviate with modern solution

## Naming

**Product name:** Prism
**Repo:** `mikalv/prism`

**Crate names:**
- `prism` (or `prism_search`) - main library
- `prism-server` - server binary
- `prism-cli` - CLI tools

## Version

Starting at `0.3.0` - significant evolution from internal component to standalone product.

## Architecture

### Repository Structure

```text
prism/
├── Cargo.toml              # Workspace root
├── README.md               # Getting started, badges
├── LICENSE                 # MIT
├── docs/
│   ├── getting-started.md
│   ├── configuration.md
│   ├── embedding-cache.md
│   ├── embedding-providers.md
│   ├── docker.md
│   ├── migration.md
│   └── api-reference.md
├── prism/                  # Main library (lib)
│   └── src/
│       ├── lib.rs
│       ├── api/            # HTTP REST API (axum)
│       ├── backends/       # text, vector, hybrid
│       ├── cache/          # embedding cache (sqlite/redis)
│       ├── collection/     # collection manager
│       ├── config/         # TOML/YAML config
│       ├── embedding/      # providers (ollama, openai, onnx)
│       ├── query/          # DSL parser (from engraph-query)
│       └── schema/         # YAML schema loader
├── prism-server/           # Server binary
│   └── src/main.rs
├── prism-cli/              # CLI tools (migrate, etc)
│   └── src/main.rs
├── Dockerfile
├── docker-compose.yml      # Prism + Ollama example
└── examples/
    ├── basic-search/
    └── rag-pipeline/
```

### Crate Organization

| Crate | Type | Purpose |
|-------|------|---------|
| `prism` | lib | Core library, published to crates.io |
| `prism-server` | bin | HTTP server binary |
| `prism-cli` | bin | Migration and admin tools |

### Dependencies

- `engraph-query` will be merged into `prism::query` (single crate distribution)
- No external llmcoder dependencies

## Embedding Cache

### Architecture

```text
┌─────────────────────────────────────────────────────────┐
│                     Prism Server                        │
├─────────────────────────────────────────────────────────┤
│  VectorBackend  →  EmbeddingCache  →  EmbeddingProvider │
│                         │                    │          │
│                    ┌────┴────┐         ┌─────┴─────┐    │
│                    │ SQLite  │         │  Ollama   │    │
│                    │ (default)│        │  OpenAI   │    │
│                    │ Redis   │         │  ONNX     │    │
│                    └─────────┘         └───────────┘    │
└─────────────────────────────────────────────────────────┘
```

### Cache Key Strategy

Configurable via `prism.toml`:

```toml
[embedding.cache]
backend = "sqlite"  # or "redis"
path = "./cache/embeddings.db"  # for sqlite
# url = "redis://localhost:6379"  # for redis

key_strategy = "model_text"  # default
# Options: "text_only", "model_text", "model_version_text"
```

### SQLite Schema

```sql
CREATE TABLE embeddings (
    key_hash     TEXT PRIMARY KEY,  -- SHA256 hex
    model        TEXT NOT NULL,
    text_hash    TEXT NOT NULL,     -- for debugging
    vector       BLOB NOT NULL,     -- f32 array as bytes
    dimensions   INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,  -- unix timestamp
    accessed_at  INTEGER NOT NULL,  -- for LRU eviction
    access_count INTEGER DEFAULT 1
);

CREATE INDEX idx_accessed ON embeddings(accessed_at);
```

### Cache Trait

```rust
#[async_trait]
pub trait EmbeddingCache: Send + Sync {
    async fn get(&self, key: &CacheKey) -> Result<Option<Vec<f32>>>;
    async fn set(&self, key: &CacheKey, vector: Vec<f32>) -> Result<()>;
    async fn stats(&self) -> CacheStats;
    async fn evict_lru(&self, max_entries: usize) -> Result<usize>;
}
```

## Embedding Providers

### Provider Trait

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn model_name(&self) -> &str;
    fn dimensions(&self) -> usize;
}
```

### Supported Providers

**1. Ollama**
```toml
[embedding.provider]
type = "ollama"
url = "http://localhost:11434"
model = "nomic-embed-text"
```

**2. OpenAI-compatible** (OpenAI, Azure, Together, Groq, etc.)
```toml
[embedding.provider]
type = "openai"
url = "https://api.openai.com/v1"
api_key = "${OPENAI_API_KEY}"
model = "text-embedding-3-small"
```

**3. ONNX (built-in)**
```toml
[embedding.provider]
type = "onnx"
model_path = "./models/all-MiniLM-L6-v2"
# Or auto-download:
model_id = "sentence-transformers/all-MiniLM-L6-v2"
```

### Feature Flags

```toml
[features]
default = ["provider-ollama"]
provider-ollama = ["reqwest"]
provider-openai = ["reqwest"]
provider-onnx = ["ort", "tokenizers", "ndarray"]
all-providers = ["provider-ollama", "provider-openai", "provider-onnx"]
```

### Optional Fallback Chain

```toml
[embedding.provider]
type = "ollama"
model = "nomic-embed-text"

[embedding.fallback]
type = "openai"
model = "text-embedding-3-small"
```

## Docker

### Dockerfile (multi-stage, distroless)

```dockerfile
# Build stage
FROM rust:1.75-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y pkg-config libssl-dev
COPY . .
RUN cargo build --release --bin prism-server

# Runtime stage (distroless)
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /build/target/release/prism-server /prism-server
COPY --from=builder /build/target/release/prism-cli /prism-cli

EXPOSE 3080
VOLUME ["/data", "/config"]

ENTRYPOINT ["/prism-server"]
CMD ["--config", "/config/prism.toml"]
```

### Image Tags

| Tag | Contents | Size |
|-----|----------|------|
| `latest`, `0.3.0` | Server + CLI, distroless | ~30-50MB |
| `0.3.0-onnx` | With ONNX runtime for local embedding | ~200MB |

### docker-compose.yml (with Ollama)

```yaml
version: "3.8"
services:
  prism:
    image: ghcr.io/mikalv/prism:latest
    ports:
      - "3080:3080"
    volumes:
      - ./data:/data
      - ./config:/config
    environment:
      - PRISM_EMBEDDING_URL=http://ollama:11434
    depends_on:
      - ollama

  ollama:
    image: ollama/ollama:latest
    volumes:
      - ollama_data:/root/.ollama
    entrypoint: ["/bin/sh", "-c", "ollama serve & sleep 5 && ollama pull nomic-embed-text && wait"]

volumes:
  ollama_data:
```

## Configuration

### prism.toml

```toml
[server]
host = "0.0.0.0"
port = 3080

[storage]
data_dir = "/data"
schemas_dir = "/config/schemas"

[embedding.provider]
type = "ollama"
url = "http://ollama:11434"
model = "nomic-embed-text"

[embedding.cache]
backend = "sqlite"
path = "/data/cache/embeddings.db"
max_entries = 1_000_000
key_strategy = "model_text"
```

## Documentation

### README.md Structure

- Badges (crates.io, docs.rs, license)
- Features list
- Quick start (Docker + from source)
- Links to detailed docs

### docs/ Contents

| File | Content |
|------|---------|
| `getting-started.md` | Installation, first collection, first search |
| `configuration.md` | Full config reference |
| `embedding-cache.md` | Cache setup, SQLite vs Redis, tuning |
| `embedding-providers.md` | Ollama, OpenAI, ONNX setup |
| `docker.md` | Docker usage, compose examples |
| `migration.md` | Migration from other systems |
| `api-reference.md` | HTTP API endpoints |

### API Documentation

- Inline rustdoc comments on public API
- Published automatically to docs.rs

## Publishing

| Platform | Location |
|----------|----------|
| Source | github.com/mikalv/prism (or prism_search) |
| Library | crates.io/crates/prism |
| Docker | ghcr.io/mikalv/prism |
| Docs | docs.rs/prism |

## Extraction Plan

### Phase 1: Preparation (in llmcoder)

1. **Copy code to new repo**
   ```bash
   mkdir ~/prism && cd ~/prism
   git init
   cp -r ~/llmcoder/platform/searchcore/* .
   cp -r ~/llmcoder/platform/engraph-query ./prism-query
   ```

2. **Restructure to workspace**
   - Create workspace Cargo.toml
   - Move library code to `prism/`
   - Create `prism-server/` and `prism-cli/`

3. **Rename and update**
   - `searchcore` → `prism`
   - `engraph-query` modules → `prism::query`
   - Update all `use` statements
   - Update Cargo.toml metadata

### Phase 2: New Features

4. **Implement embedding cache**
   - `prism/src/cache/mod.rs`
   - `prism/src/cache/sqlite.rs`
   - `prism/src/cache/redis.rs`
   - `prism/src/cache/trait.rs`

5. **Refactor embedding providers**
   - `prism/src/embedding/provider.rs` (trait)
   - `prism/src/embedding/ollama.rs`
   - `prism/src/embedding/openai.rs`
   - `prism/src/embedding/onnx.rs` (existing code)

### Phase 3: Production Readiness

6. **Documentation**
   - README.md with badges, quick start
   - docs/ folder with guides
   - Rustdoc comments on public API

7. **CI/CD**
   - GitHub Actions: test, lint, build
   - Automatic publishing to crates.io
   - Docker build and push to ghcr.io

8. **Examples**
   - `examples/basic-search/`
   - `examples/rag-pipeline/`

### Phase 4: Cleanup in llmcoder

**Safety Gate - Before Phase 4:**
- [ ] Prism repo builds and all tests pass
- [ ] Published to crates.io (at least one version)
- [ ] Docker image pushed to ghcr.io
- [ ] mnemos-daemon tested with `prism` from crates.io in a branch
- [ ] Verified working for 1-2 weeks

9. **Replace searchcore with prism dependency**
   ```toml
   # llmcoder/platform/Cargo.toml
   [workspace.dependencies]
   prism = "0.2"  # from crates.io
   ```

10. **Remove old code**
    - Delete `platform/searchcore/`
    - Delete `platform/engraph-query/`
    - Update imports in mnemos-daemon etc.

## Summary

| Aspect | Decision |
|--------|----------|
| **Name** | Prism |
| **Target** | AI/RAG developers + enterprise |
| **Version** | 0.3.0 |
| **Repo** | mikalv/prism |
| **Publishing** | crates.io + ghcr.io |
| **Embedding cache** | SQLite (default) + Redis (optional) |
| **Cache key** | Configurable, default `SHA256(model + text)` |
| **Providers** | Ollama + OpenAI-compatible + ONNX |
| **engraph-query** | Merged into prism::query |
| **Docker** | Distroless, single binary, ~30-50MB |
| **Documentation** | README + docs/ + docs.rs |

## Decisions Made

- [x] **Repo name:** `mikalv/prism`
- [x] **Starting version:** `0.3.0`
- [ ] **License:** MIT (same as current)?
