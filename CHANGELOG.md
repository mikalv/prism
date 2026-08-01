# Changelog

All notable changes to Prism are documented in this file.

## [Unreleased]

### Added

- **`prism-mailsync` client (`clients/python/mailsync`)** — an incremental,
  resumable indexer that syncs a remote **IMAP** mailbox (many folders, very
  large) into a Prism collection via the ES-compatible `/_elastic/_bulk`
  endpoint, making all mail full-text searchable. Tracks a per-folder
  `UIDVALIDITY`+UID watermark in SQLite so re-runs only fetch new mail and a
  crash resumes where it stopped; uses `Message-ID` as the document `_id` for
  cross-folder dedup and idempotent re-runs. Supports password and OAuth2
  (XOAUTH2) auth, implicit SSL / STARTTLS, and an optional built-in SSH tunnel
  to the IMAP host. `uv`-managed with 50 unit tests; verified end-to-end
  (create → bulk → search → cleanup) against the live 0.6.10 server.
- **Embedded web UI is now shippable to prod** — the `websearch-ui` React app is
  compiled into the server binary (via `prism-ui`/`rust-embed`) and served at
  `/ui/`. It was previously dropped from prod builds because `build.rs` needs
  `npm` to build `websearch-ui/dist` and the server host has no Node. The build
  only runs npm when `dist/index.html` is missing, so a pre-built `dist` can be
  shipped as a deploy artifact and embedded with the `ui` feature — no Node
  required on the build host.

### Fixed

- **`/api/search` with no collection now searches ALL collections** — the web
  UI's default "All" search (`simple_search`) fell back to the first registered
  collection when no collection was specified, so it silently searched one
  arbitrary collection. A query with 1600+ hits in the `mail` collection could
  return nothing. It now uses `multi_search` (RRF-merged) across every
  registered collection; an explicit collection still searches just that one.
- **Collection dropdown is searchable, scrollable, and height-capped** — with
  many collections (83 on prod) the selector rendered an uncapped list. It now
  has a filter input (autofocused; Enter picks the first match, Esc closes) and
  a fixed max-height scrollable list with an empty state.
- **Web UI static shell no longer requires authentication** — the auth
  middleware whitelist covered only `/health` and `/stats/server`, so with
  security enabled every `/ui/*` asset (including `index.html`) returned `401`
  and the UI could never render to prompt for a key. `/ui` is now whitelisted
  (extracted into a tested `is_public_path` helper); the UI's data requests
  (`/api/*`, `/collections/*`, `/admin/*`) still authenticate normally.
- **`websearch-ui` production API base URL** — `.env.production` set
  `VITE_API_URL=/api`, which combined with the code's own `/api/...` prefix to
  produce broken `/api/api/...` request paths. It is now empty so the embedded
  UI calls the API on the same origin (`/api/search`, `/collections/:c/search`,
  `/admin/collections`).

## [0.6.10] - 2026-07-17

Elasticsearch-compatibility and correctness release: adds `exists` queries,
`_source` filtering (with wildcards), accurate `hits.total.relation`,
collector-level sorting, exact aggregations, and vector pagination; hardens
collection deletion (no data resurrection) and admin-endpoint authorization.

### Fixes

- **Vector search now honors `offset`** — kNN/vector pagination previously ignored `offset` (every page returned the same top results). The backend now fetches `offset + limit` candidates and skips `offset`, so paginated vector and hybrid queries return distinct pages.
- **Aggregations are exact beyond the 10k window** — aggregation buckets and metrics were computed over only the top-10,000 documents used for hits. They now run over the full matching document set (via a `DocSetCollector`, only when aggregations are requested), so counts, sums, terms buckets, etc. are correct regardless of match count.

### Added

- **ES-compat `_source` wildcard patterns** — `_source` include/exclude entries now support `*` wildcards (`"*"`, `"user.*"`, `"_*"`, `"*_id"`, `"a*c"`) in addition to exact names, so requests can select or drop groups of fields (e.g. `{"excludes": ["_*"]}` to hide internal fields). Dotted names match flat field names literally, since Prism stores fields flat.

### Security

- **`/_admin/*` endpoints now require the Admin permission** — the auth middleware only gated `/admin/*` (no underscore), so the `/_admin/*` administrative endpoints (encrypted export/import, collection detach/attach, encryption-key generation) were reachable by any authenticated API key regardless of role. When security is enabled, these now require the global Admin permission like `/admin/*`. The permission-check logic was also deduplicated into a single tested helper shared by both middleware variants.

### Fixes

- **`DELETE /collections/:name` now deletes on-disk data** — previously the endpoint only unloaded the collection from memory and removed its schema file, leaving the index data on disk. Recreating a collection with the same name resurrected the supposedly-deleted documents. The endpoint now purges the collection's data directory by default (matching Elasticsearch's `DELETE /index`); pass `?delete_data=false` to unload while keeping the data. The vector backend also no longer re-saves its index when a collection is removed (which had reinforced the resurrection), and the `detach` API's data-deletion path — which pointed at a non-existent `collections/` subdirectory and silently deleted nothing — now removes the correct directory.

### Added

- **Collector-level sort (no scan cap) for fast fields** — a single-key sort on a fast numeric/date/bool field is now performed at the Tantivy collector level, fetching only the requested page instead of scanning up to 10,000 documents and sorting in memory. This removes the result-window cap for that common case (e.g. "last N by timestamp") and is dramatically faster on large collections. Missing values still sort last, matching the previous behavior. Multi-key sorts, `_score` sorts, string-field sorts, highlighting-with-sort, and sorts on non-fast fields keep using the in-memory scan path.
- **`exists` query support (core + ES-compat)** — `Query` gains `exists_fields` / `not_exists_fields`, and the text backend applies them via Tantivy's fast-field `ExistsQuery` inside a boolean query. The ES `_search` translator extracts `exists` clauses (standalone, and inside a `bool`'s `must`/`filter` → required, `must_not` → forbidden) into these lists, so the standard Kibana "field exists / does not exist" filter now works instead of returning a 400. Requires the field to be a fast field: newly created collections mark numeric/date/bool **and string** fields fast; existing collections must be recreated to enable exists on their fields (a clear error is returned otherwise). The cluster RPC query propagates the exists fields across nodes.
- **ES-compat accurate `hits.total.relation`** — `_search`/`_msearch` now report `relation: "gte"` (with the value clamped to the 10,000 result window) when a collection has more matching documents than were counted, instead of always claiming `"eq"`. A 30k-document collection now returns `{"value": 10000, "relation": "gte"}` rather than a misleading `{"value": 10000, "relation": "eq"}`. The text backend fetches one past the window to distinguish "exactly 10,000" from "more than 10,000", and `track_total_hits: <n>` lowers the tracking limit.
- **ES-compat `_source` filtering** — the `_source` parameter is now honored on `_search` and `_msearch`. `_source: false` omits the `_source` key entirely; a field list (`_source: ["a", "b"]`) or object form (`{"includes": [...], "excludes": [...]}`) selects which stored fields are returned. Field names are matched exactly (dot-path and wildcard selection are not yet supported).

## [0.6.9] - 2026-07-16

Elasticsearch-compatibility hardening: this release makes the `/_elastic`
layer usable from official ES clients and fixes several query shapes and
core search correctness bugs, plus adds first-class result sorting.

### Added

- **Result sorting** — `Query` now carries a `sort` field (a list of `SortField { field, ascending }`), and the text backend honors it: results are sorted by the given keys (the special `_score` key sorts by relevance), with missing values last. Sorting scans up to `SORT_SCAN_CAP` (10,000) matching documents, matching Elasticsearch's `index.max_result_window`. The ES-compat `_search` endpoint maps the `sort` clause onto this (a bare field defaults to ascending, except `_score`), and the cluster RPC query propagates `sort` across nodes. Newly created collections now mark numeric/date/bool fields as fast fields to enable future collector-level sorting.

### Fixes

- **ES-compat `X-Elastic-Product` header** — every `/_elastic/*` response now sends `X-Elastic-Product: Elasticsearch` (on success and error responses). Official Elasticsearch clients (elasticsearch-py/js/java ≥ 7.14) verify this header and previously refused to connect with `UnsupportedProductError`.
- **ES-compat `_search` on hybrid collections** — `HybridSearchCoordinator::search_with_aggs` no longer returns `NotImplemented`, so `POST /_elastic/{index}/_search` against a collection with both text and vector backends works instead of returning **HTTP 500**. Without aggregations it runs the normal hybrid search; with aggregations it delegates bucket computation to the text backend.
- **ES-compat `bool` queries** — `must`/`filter`/`must_not`/`should` now translate to Tantivy occur syntax (`+`/`-`/bare) instead of `AND`/`NOT` string joins. This fixes the standard Kibana filter-with-exclusion (`must` + `must_not`) which previously returned **zero results**, and a `must_not`-only query which previously returned **HTTP 500**. `should` is now correctly optional when a `must`/`filter` is present.
- **ES-compat `match` / `multi_match`** — multi-word values are analyzed into OR-combined terms (`field:(a b)`) instead of being quoted as a phrase, so `{"match": {"content": "connection timeout"}}` matches docs containing either term (ES semantics). `operator: "and"` requires all terms; `multi_match` combines fields with SHOULD (best_fields).
- **ES-compat `ids` query** — looks up the document id in the `id` field instead of the nonexistent `_id` field (previously returned HTTP 500 / no matches).
- **ES-compat `exists` query** — returns a clean `400 parsing_exception` instead of a `500`, since `field:*` is not a valid engine query (true `exists` support is pending a structured backend query).
- **Correct `total` hit count** — text search now runs a `Count` collector alongside `TopDocs`, so `total` reflects the true number of matching documents instead of the truncated page size. Previously `limit=1` reported `total=1` for a query with hundreds of matches, breaking pagination and "N results" UIs (affected `/collections/:c/search`, `/api/search`, and ES-compat `hits.total`)
- **`limit=0` no longer panics** — the Tantivy `TopDocs::with_limit` fetch size is clamped to at least 1 (it panics on 0), and the result loop respects the requested page size, so `{"limit":0}` / ES `{"size":0}` returns a count-only response instead of aborting the request. Also guards against `limit + offset` overflow via `saturating_add`
- **`/api/search` honors the `collection` field** — the simple-search endpoint now searches the requested collection (404 if unknown) instead of always querying `list_collections().first()`, which was nondeterministic (HashMap order) and ignored the caller's `collection`

## [0.6.8] - 2026-03-15

### Performance

- **174x faster startup** — HNSW vector indexes now serialize the full graph structure (layers, neighbor connections) via bincode, eliminating O(n log n) graph rebuild on load. Startup with 65 vector indexes: 1220s → 7.6s
- **Parallel collection loading** — vector and graph backends initialize concurrently via `tokio::spawn` instead of sequentially
- **Binary HNSW format** — new V2 format ("PRH2" magic) with auto-detection; falls back to V1 binary ("PRHW") and legacy JSON with automatic re-persist
- **Lazy IndexWriter** — tantivy `IndexWriter` is created on first write, not on collection load. Read-only collections use zero writer threads
- **NoMergePolicy** — disables tantivy's background merge threads; segment merging handled by Prism's own optimize cycle
- **Single-threaded writer** — `writer_with_num_threads(1, heap)` with dynamic heap sizing (15MB small / 50MB large schemas)
- **Smarter segment merging** — small collections (<1000 docs) always merge to 1 segment; merge triggered on >20% delete ratio

### Fixes

- **HTTP error codes** — `CollectionNotFound` returns 404, `CollectionAlreadyExists` returns 409, `InvalidQuery`/`Schema` returns 400, `Unauthorized` returns 401, `ReadOnly` returns 403 (were all 500)
- **Export/import path** — corrected collection data path resolution
- **CLI list_collections** — fixed wrong subdirectory path
- **Migrate command** — uses correct `Document {id, fields}` format

### Added

- **Document scroll endpoint** — `GET /collections/:name/documents/scroll` for paginated document export
- **Schema raw endpoint** — `GET /collections/:name/schema/raw` returns the original schema definition
- **CLI migrate command** — migrate collections between Prism instances
- **Tree-sitter feature** — exposed `tokenizer-treesitter` in prism-server for production builds

---

## [0.6.7] - 2026-03-10

### Security

- **Security enabled by default** — `[security] enabled = true` is now the default; API keys must be configured or security explicitly disabled for development
- **Path traversal protection** — `StoragePath` rejects `../` sequences in collection names and document IDs, with defense-in-depth assertions
- **Constant-time API key comparison** — prevents timing side-channel attacks on authentication
- **StoragePath deserialization bypass fix** — closes a vulnerability where crafted deserialized paths could skip validation

### Added

- **Async indexing queue** — `POST /collections/:col/documents` now returns `202 Accepted` immediately, processing in background via `tokio::mpsc` channel. Falls back to synchronous `201 Created` when queue is full. Use `?sync=true` to force synchronous indexing
- **ES document CRUD endpoints** — `GET/PUT/POST/DELETE /_elastic/{index}/_doc/{id}`, `HEAD /_elastic/{index}/_doc/{id}`, `HEAD /_elastic/{index}`, `GET /_elastic/{index}/_count`, `GET /_elastic/{index}/_search?q=...` for Elasticsearch client compatibility
- **Embedding error propagation** — embedding generation failures now return clear errors instead of cryptic "Missing embedding field" messages

### Fixes

- **Dimension mismatch detection** — persisted vector indexes with wrong dimensions are detected on load instead of silently corrupting search results
- **Embedding text truncation** — texts are truncated to 2000 chars before embedding to prevent Ollama context overflow
- **Dynamic batch splitting** — large embedding batches are automatically split when they exceed provider limits
- **Server bind_addr from config** — `[server] bind_addr` in config file is now respected (was previously ignored in favor of CLI args only)

### Documentation

- Updated API reference with async indexing (202/201), `?sync=true` param, proper request format
- New Elasticsearch Compatibility guide (`docs/guides/elasticsearch-compat.md`)
- Updated security docs for enabled-by-default, constant-time auth, path traversal protection
- Updated configuration docs for security default change
- Elixir client (`prismsearch`) hex.pm metadata: description, maintainers, links to Prism repo and docs

### Client Libraries

- **Elixir** — hex.pm-ready metadata with proper description, links, and README

---

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
