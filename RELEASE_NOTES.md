# Prism v0.6.7 Release Notes

**Release date:** 2026-03-10

## Highlights

Security hardening (enabled by default, path traversal protection, constant-time auth), async indexing queue for non-blocking document ingestion, Elasticsearch document CRUD endpoints, and embedding reliability fixes.

## Security

This release makes Prism **secure by default**. Security is now enabled out of the box — you must configure API keys or explicitly set `enabled = false` for local development. Three additional hardening measures were added:

- **Path traversal protection** — StoragePath validates all paths, rejecting `../` sequences
- **Constant-time API key comparison** — prevents timing attacks on authentication
- **Deserialization bypass fix** — closes a StoragePath validation bypass via crafted deserialized data

## Async Indexing Queue

Document indexing no longer blocks the HTTP response. `POST /collections/:col/documents` now returns **202 Accepted** immediately, with documents processed in the background via a `tokio::mpsc` channel (capacity: 1000 jobs).

- **202 Accepted** — documents queued for background indexing (default)
- **201 Created** — synchronous fallback when queue is full (no data loss)
- **`?sync=true`** — query parameter to force synchronous indexing

The response includes a `"queued": true` field so clients can distinguish async from sync processing.

## Elasticsearch Compatibility

New document-level CRUD endpoints for ES client compatibility:

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/_elastic/{index}/_doc/{id}` | Get document |
| `HEAD` | `/_elastic/{index}/_doc/{id}` | Check document exists |
| `POST` | `/_elastic/{index}/_doc` | Index with auto-ID |
| `PUT` | `/_elastic/{index}/_doc/{id}` | Index with explicit ID |
| `DELETE` | `/_elastic/{index}/_doc/{id}` | Delete document |
| `HEAD` | `/_elastic/{index}` | Check index exists |
| `GET` | `/_elastic/{index}/_count` | Count documents |
| `GET` | `/_elastic/{index}/_search?q=...` | Query string search |

These complement the existing `_search`, `_bulk`, `_mapping`, `_msearch`, and `_cat` endpoints.

## Embedding Reliability

- **Error propagation** — embedding failures now surface as clear errors instead of "Missing embedding field"
- **Text truncation** — inputs automatically truncated to 2000 chars to prevent Ollama context overflow
- **Dynamic batch splitting** — large batches auto-split when they exceed provider limits
- **Dimension mismatch detection** — persisted indexes with wrong dimensions are caught on load

## Bug Fixes

- `[server] bind_addr` from config file is now respected (was ignored in favor of CLI-only args)

## Documentation

- New: [Elasticsearch Compatibility guide](docs/guides/elasticsearch-compat.md)
- Updated: API reference, security docs, configuration docs
- Updated: Elixir client hex.pm metadata for publishing

## Migration from v0.6.6

1. **Security is now on by default.** Either configure API keys in `prism.toml` or add `[security] enabled = false` for development.
2. **Indexing returns 202 instead of 201.** Update clients to accept both status codes, or use `?sync=true` for the previous behavior.
3. No schema or data format changes — fully backwards compatible.
