# Changelog

All notable changes to Prism are documented in this file.

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
