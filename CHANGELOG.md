# Changelog

All notable changes to Prism are documented in this file.

## [0.6.6] - 2026-02-23

### Security

- **CRITICAL: Authentication bypass on ES-compat routes** — `Router::merge()` was not propagating auth/audit middleware to extension routes; refactored middleware to apply after route merging
- **Bulk request limits** — `MAX_BULK_ACTIONS=10,000` rejects oversized bulk requests
- **Query string length validation** — `MAX_QUERY_STRING_LENGTH=10,000` prevents DoS via passthrough queries
- **Search result limits** — `MAX_SEARCH_LIMIT=10,000` caps across Lucene, Mnemos, and ES-compat endpoints
- **Lucene parser stack overflow** — `MAX_PARSE_DEPTH=50` recursion limit prevents deeply nested query attacks
- **Parenthesis underflow fix** — unbalanced `)` treated as literal instead of causing integer underflow
- **Wildcard bulk index rejection** — `*`/`?` patterns in bulk index names now return error
- **Error sanitization** — internal error details logged server-side, generic messages returned to clients

### Fixes

- **Segment merge race condition** — cache segment data in `StorageFileHandle` to eliminate file-not-found errors during concurrent merges
- **ES-compat 404 for missing collections** — `CollectionNotFound` now returns HTTP 404 instead of 500
- **Collection removal race** — all four write locks acquired atomically to prevent partial state

### Performance

- **LRU cache multi-eviction** — eviction now loops until enough space is freed for large entries
- **parking_lot::RwLock** — LRU cache switched from `std::sync::RwLock` for better contention handling
- **Zero-copy bool query translation** — `QueryList::iter()` eliminates cloning in ES-compat query translation

### Added

- **Synchronous segment merge** — `POST /collections/:name/optimize` endpoint for on-demand segment consolidation
- Comprehensive test coverage across all crates

### Housekeeping

- Deduplicated `get_text_fields` between search.rs and msearch.rs
- Replaced nightly-only `is_multiple_of` with stable modulo operator
- Removed dead wrapper functions from Lucene parser
- Removed unused `mut` warnings

## [0.6.5] - 2026-02-18

### Performance

- **Batch embedding cache ops** — `get_batch()`/`set_batch()` on `EmbeddingCache` trait reduce 500-doc bulk import from ~1000 DB operations to ~2
- **SQLite WAL mode** — `journal_mode=WAL`, `synchronous=NORMAL`, 64MB page cache for 2-5x write throughput
- **Chunked provider calls** — configurable `batch_size` (default 128) splits large embedding requests into provider-friendly chunks
- **Redis batch ops** — `MGET` for batch reads, pipelined writes for Redis cache backend

### Configuration

- `embedding.batch_size` — max texts per embedding API call (default: 128)
- `embedding.concurrency` — max concurrent embedding API calls (default: 4)

### Documentation

- Updated embedding config docs to match actual `[embedding.provider]` structure
- Added bulk import tuning guide with `batch_size`/`concurrency` knobs
- Fixed incorrect defaults for `cors.enabled`, `metrics_enabled`, `data_dir`

## [0.6.4] - 2026-02-15

### Fixes

- **Tantivy "Path not found" crash** — disable background merge threads (`NoMergePolicy`) to prevent segment file deletion races in `TantivyStorageAdapter`
- **Stale reader segments** — switch to `ReloadPolicy::Manual` with explicit `reader.reload()` before every search
- **413 Payload Too Large** — configurable `max_body_size` (default 100MB) via `ServerConfig`

### Tests

- 7 new concurrent text backend tests: interleaved index/search, parallel tasks, bulk indexing, delete+reindex

## [0.6.3] - 2026-02-13

### Highlights

Zero-downtime rolling cluster upgrades with protocol version negotiation and node drain/undrain.

### Cluster

- **Rolling Upgrade Support** ([#39](https://github.com/mikalv/prism/issues/39)) — protocol version negotiation at heartbeat level enables mixed-version clusters during upgrades
- **Node Drain/Undrain** ([#39](https://github.com/mikalv/prism/issues/39)) — administrative drain state stops routing queries to a node while keeping it alive for graceful upgrades
- **Federation Routing** — query router skips draining nodes, falling back to replicas automatically
- **Upgrade Status API** — `GET /cluster/upgrade/status` shows version and drain state of all nodes

### CLI

- **`prism cluster upgrade-status`** — display cluster-wide version and drain status
- **`prism cluster drain --node <id>`** — drain a node before upgrade
- **`prism cluster undrain --node <id>`** — resume routing after upgrade

## [0.6.2] - 2026-02-13

### Highlights

crates.io publishing as `prismsearch`, macOS code signing, native ARM builds, graph merge CLI.

### Graph

- **Graph Shard Merge CLI** ([#41](https://github.com/mikalv/prism/issues/41)) — `prism collection graph-merge` consolidates all graph shards into shard 0 for full cross-shard traversal
- **Collection Merge CLI** ([#41](https://github.com/mikalv/prism/issues/41)) — `prism collection merge` combines graph data from multiple collections into a new target
- **Sharded Graph Backend with HTTP API** ([#41](https://github.com/mikalv/prism/issues/41)) — distributes graph nodes across shards, BFS/shortest-path, full CRUD via REST

### Server & API

- **Create/Delete Collection Endpoints** ([#76](https://github.com/mikalv/prism/issues/76)) — runtime collection management via `POST /collections` and `DELETE /collections/:name`

### Packaging & CI

- **crates.io publishing** — all crates renamed to `prismsearch-*` (Rust import paths unchanged)
- **macOS code signing** — binaries signed with hardened runtime via Apple Developer certificate
- **Native ARM builds** — switched from cross-compilation to `ubuntu-24.04-arm` runners
- **Binary stripping** — Linux binaries stripped for smaller release archives

### Fixes

- ES-compat: fix axum 0.7 path param syntax for index routes

### Documentation

- Graph search feature guide with sharding, API reference, and merge operations
- Updated CLI reference with graph-merge and merge commands

### Breaking Changes

None — backwards compatible with v0.6.0.

---

## [0.6.1] - 2026-02-12

### Highlights

Graph sharding merge commands, collection management API, and documentation improvements.

### Graph

- **Graph Shard Merge CLI** ([#41](https://github.com/mikalv/prism/issues/41)) — `prism collection graph-merge` consolidates all graph shards into shard 0 for full cross-shard traversal
- **Collection Merge CLI** ([#41](https://github.com/mikalv/prism/issues/41)) — `prism collection merge` combines graph data from multiple collections into a new target
- **Sharded Graph Backend with HTTP API** ([#41](https://github.com/mikalv/prism/issues/41)) — distributes graph nodes across shards, BFS/shortest-path, full CRUD via REST

### Server & API

- **Create/Delete Collection Endpoints** ([#76](https://github.com/mikalv/prism/issues/76)) — runtime collection management via `POST /collections` and `DELETE /collections/:name`

### Documentation

- Graph search feature guide with sharding, API reference, and merge operations
- Updated CLI reference with graph-merge and merge commands
- MkDocs navigation updated

### Breaking Changes

None — backwards compatible with v0.6.0.

---

## [0.6.0] - 2026-02-11

### Highlights

AST-aware code search, advanced ranking, HNSW sharding, web UI, cluster fixes.

### Code Search

- **Tree-sitter AST Code Tokenizer** ([#70](https://github.com/mikalv/prism/issues/70)) — 16 languages, identifier splitting
- Code search documentation and schema reference

### Ranking & Search Quality

- **Advanced Hybrid Ranking** ([#56](https://github.com/mikalv/prism/issues/56)) — score normalization, per-query controls
- **Two-Phase Ranking** ([#52](https://github.com/mikalv/prism/issues/52)) — pluggable re-rankers

### Vector & Storage

- **HNSW Index Sharding** ([#40](https://github.com/mikalv/prism/issues/40)) — segments, compaction, bitmap tombstones

### Server & API

- Live Collection Detach/Attach ([#57](https://github.com/mikalv/prism/issues/57))
- Embedded Web UI at `/ui` (enabled by default)
- Collection selector dropdown, search-only mode
- Root endpoint with version/status
- `PRISM_LOG_DIR`, `PRISM_CACHE_DIR` env vars

### Cluster

- Federated search fix — QUIC with Json serde, stream-per-call
- 3-node Docker Compose integration test

### Importer

- Wikipedia XML dump source

### Documentation

- Code Search guide
- Clustering & Federation guide
- Updated README

### Breaking Changes

None — backwards compatible with v0.5.0.

---

## [0.5.0] - 2026-02-07

### Highlights

This release introduces **distributed clustering**, **encryption at rest**, **Elasticsearch compatibility**, and **Index Lifecycle Management** - making Prism production-ready for enterprise deployments.

### Clustering & Distribution

- **Inter-node RPC Protocol** ([#32](https://github.com/mikalv/prism/issues/32)) — tarpc/bincode over QUIC for low-latency cluster communication
- **Node Discovery** ([#29](https://github.com/mikalv/prism/issues/29)) — Static and DNS-based service discovery
- **Health Checks** ([#36](https://github.com/mikalv/prism/issues/36)) — Node membership monitoring and failure detection
- **Replication & Shard Placement** ([#33](https://github.com/mikalv/prism/issues/33)) — Zone-aware replica placement with load balancing
- **Federation Layer** ([#28](https://github.com/mikalv/prism/issues/28)) — Query routing and result merging across nodes
- **Schema Versioning** ([#35](https://github.com/mikalv/prism/issues/35)) — Versioned schema propagation across cluster
- **Split-brain Detection** ([#37](https://github.com/mikalv/prism/issues/37)) — Network partition handling with quorum-based decisions
- **Cluster Observability** ([#69](https://github.com/mikalv/prism/issues/69)) — Prometheus metrics for cluster health

### Security & Encryption

- **AES-256-GCM Encryption** ([#75](https://github.com/mikalv/prism/issues/75)) — Application-level encryption at rest
  - Storage-level encryption via configuration
  - Runtime encryption via HTTP API (no restart needed)
  - Key management: environment variables, hex, base64
- **Encrypted Export/Import** — Secure backup to untrusted cloud storage
- **SIGHUP Config Reload** — Hot-reload security settings without restart

### Index Lifecycle Management

- **ILM Policies** ([#45](https://github.com/mikalv/prism/issues/45)) — Automatic index rollover and retention
  - Phase transitions: hot → warm → cold → frozen → delete
  - Rollover triggers: size, age, document count
  - Storage tier migration (local → S3)
- **Index Templates** ([#51](https://github.com/mikalv/prism/issues/51)) — Auto-apply settings to new indices
- **Aliases** ([#50](https://github.com/mikalv/prism/issues/50)) — Virtual names for zero-downtime reindexing

### Elasticsearch Compatibility

- **ES API Layer** ([#73](https://github.com/mikalv/prism/issues/73)) — Drop-in replacement for ES clients
  - `/_bulk` endpoint for bulk indexing
  - `/_search` with query DSL subset
  - `/_cat` endpoints for cluster info
  - Index and document CRUD operations

### Storage & Performance

- **LZ4/Zstd Compression** ([#71](https://github.com/mikalv/prism/issues/71)) — Transparent compression for on-disk data
  - LZ4: fastest, ~2x compression
  - Zstd: balanced, ~3x compression
  - Configurable compression levels
- **Multi-Collection Search** ([#74](https://github.com/mikalv/prism/issues/74)) — Query multiple indices in one request
  - `/_msearch` endpoint
  - `/:collections/_search` with comma-separated names
  - Wildcard patterns: `logs-*`

### Export & Backup

- **Collection Export/Import** ([#30](https://github.com/mikalv/prism/issues/30))
  - Portable format: JSON/NDJSON, cross-version compatible
  - Snapshot format: tar.zst binary, fast backup/restore
  - Encrypted format: AES-256-GCM for secure cloud storage
- **CLI Commands**: `prism-cli collection export/restore`
- **API Endpoints**: `/_admin/export/encrypted`, `/_admin/import/encrypted`

### Developer Experience

- **Code Tokenizer** ([#66](https://github.com/mikalv/prism/issues/66)) — Code-aware tokenization for source code search
  - CamelCase and snake_case splitting
  - Identifier extraction
- **ONNX Embeddings** — Local embedding generation with auto-download
- **Pluggable Providers** — Ollama, OpenAI, ONNX for embeddings
- **Service Installers** — launchd (macOS) and systemd (Linux) scripts
- **Static Linux Builds** — musl-based binaries for any Linux

### Bug Fixes

- Fixed Docker image missing prism-importer binary
- Fixed ONNX feature flags not forwarding to prism crate
- Fixed CLI --schemas-dir argument being ignored
- Fixed duplicate tracing subscriber initialization

### Documentation

- New: [Encryption Guide](docs/guides/encryption.md)
- New: [Export & Import Guide](docs/guides/export-import.md)
- Updated: Storage Backends with encryption and compression
- Updated: API Reference with new endpoints

### Breaking Changes

None - this release is backwards compatible with v0.4.0 configurations.

### Migration from v0.4.0

1. Update binaries
2. (Optional) Enable new features in `prism.toml`:
   - `[storage.encrypted]` for encryption
   - `[storage.compressed]` for compression
   - `[ilm]` for lifecycle management
   - `[cluster]` for distributed mode

---

## [0.4.0] - 2026-02-05

Initial public release with:
- Hybrid search (text + vector)
- Tantivy full-text backend
- HNSW vector backend
- REST API
- MCP (Model Context Protocol) support
- Security: API keys, RBAC, audit logging
- S3 storage backend
- Ingest pipelines
- Highlighting, suggestions, more-like-this
- prism-server, prism-cli, prism-import tools
