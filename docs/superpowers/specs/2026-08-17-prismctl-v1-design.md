# prismctl V1 — Full API Client CLI

Date: 2026-08-17
Status: Approved design (pending implementation plan)
Scope: Option A (core), with B (cross-server copy/move) as immediate follow-up.

## Purpose

`prismctl` is the first-class command-line client for the Prism HTTP API. It
exposes every capability of the API — search, document operations, schema
management, reindex, backup/restore, graph — with human-friendly table output
by default and machine-readable `--output json` for scripting. It complements
(rather than replaces) the existing file-mode commands that operate directly
on a data directory with the server stopped.

## Decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Structure | Hybrid: new API-based top-level commands + legacy file commands kept under `collection`/`cluster` |
| Command style | Resource-oriented (verb-first, ES/kubectl style) |
| Default output | Human table; `-o json` / `--output json` for raw API JSON |
| Server address | `--url` flag + `PRISM_URL` env var; `PRISM_API_KEY` env for auth |
| V1 scope | Core (search, doc, schema, reindex, collections list, backup/restore, graph) |
| Follow-up | Cross-server copy/move (V1.1), cluster command migration |

## Architecture

One binary, two modes, clearly separated:

```
prismctl
├── API mode (new, first-class): search, doc, schema, reindex,
│   collections, backup, restore, graph
│   └── --url / PRISM_URL env; PRISM_API_KEY for auth
└── File mode (legacy, unchanged): collection inspect/migrate/merge/optimize,
    cluster ..., benchmark, cache-stats/clear, snapshot restore
    └── --data-dir, requires stopped server
```

Nothing existing is removed or changed in file mode.

## Components

### 1. `prism-cli/src/client.rs` (~250 lines)

HTTP client, one method per API operation:

- `PrismClient::new(url, api_key) -> Result<Self>` — reqwest-based,
  validates URL, `PRISM_API_KEY` (or `--api-key`) becomes Bearer header
- Timeouts: 30s default, `--timeout` flag (seconds) per command; global
- Retries: 3 attempts with exponential backoff on 5xx/network errors; never
  retries 4xx
- Typed serde request/response structs for each operation; errors surface
  HTTP status + server message via `anyhow`

Methods (V1):
- `collections_list() -> Vec<CollectionInfo>` (`GET /admin/collections`)
- `search(collection, query, mode, limit)` → `POST /:collection/_search`
  with ES-compat body; mode maps to query body shape
- `get_document(collection, id)` → `GET /:collection/documents/:id`
- `index_document(collection, doc)` → `POST /:collection/documents`
- `delete_document(collection, id)` → `DELETE /:collection/documents/:id`
- `bulk_index(collection, docs)` → `POST /:collection/documents/_bulk`
  (or batched `POST /:collection/documents`; use whichever the server
  supports — verify during implementation)
- `get_schema / apply_schema / lint_schemas` → `/schema`, `/schema`,
  `/admin/lint-schemas`
- `reindex(collections, batch_size)` → `POST /admin/reindex`
- `backup(collection, path)` → stream from `/admin/export/encrypted` (or
  plain snapshot endpoint; verify exact contract during implementation)
- `restore(path, collection)` → `POST /admin/import/encrypted`
- `graph_edges / graph_bfs / graph_path / graph_stats` →
  `/graph/edges`, `/graph/bfs`, `/graph/shortest-path`, `/graph/stats`

### 2. `prism-cli/src/output.rs` (~150 lines)

- `print_collections(table)`: name, docs, size (human-readable), embedding
- `print_search_results(table)`: rank, score, id, and top fields (title/url
  if present, else first text-field snippet, truncated to ~80 chars)
- `print_document`: key/value listing (one field per line)
- `print_reindex_summary`: per-collection reembedded/skipped + totals
- `print_schema`: YAML dump of schema (via serde_yaml)
- Graph: `print_graph_edges`: node → node (weight); `print_graph_bfs`:
  node list with depth; `print_graph_path`: chain `a -> b -> c`
- JSON mode: pass through server JSON unmodified (`serde_json::Value`)
- Color on TTY only, `--no-color` / `NO_COLOR` env

### 3. Command tree (main.rs)

New top-level commands (API mode):

```
prismctl search <collection> <query> [--mode hybrid|vector|text] [--limit N]
prismctl doc get <collection> <id> [-o json]
prismctl doc index <collection> (<file> | -)          # - reads stdin JSON
prismctl doc delete <collection> <id>
prismctl doc bulk <collection> (<file.jsonl> | -)     # - reads stdin JSONL
prismctl schema get <collection>
prismctl schema apply <collection> <schema-file>
prismctl schema lint
prismctl reindex [--collections "idx_*" | --all] [--batch-size 100]
prismctl collections
prismctl backup <collection> <file.prism>
prismctl restore <file.prism> <collection>
prismctl graph edges <collection> <node>
prismctl graph bfs <collection> <node> [--depth N]
prismctl graph path <collection> <from> <to>
prismctl graph stats <collection>
prismctl suggest <collection> <prefix>              # /_suggest (same
                                                     # endpoint, type=completion)  
```

Global flags: `--url`, `--api-key`, `--output|-o` (table|json), `--no-color`,
`--timeout`, `--insecure` (skip TLS verification for self-signed certs).

Legacy file-mode commands unchanged: `collection`, `cluster`, `benchmark`,
`cache-stats`, `cache-clear`.

## Data Flow

Request flow: clap parse → `PrismClient::new(url, api_key)` → method call →
typed response → `output.rs` render (table default, JSON on `-o json`).

Error paths: connection refused → hint "is the server running at <url>?
Set --url or PRISM_URL"; 404 → collection not found hint; 4xx → server
message verbatim; 5xx retried then surfaced.

## Error Handling

- Client errors (bad URL, connection refused) exit code 1 with actionable hint
- Server 4xx: print server error message, exit 2
- Server 5xx after retries: print status + body, exit 3
- `doc index -` with empty stdin: error, not hang
- `backup` writes to temp file then renames (atomic-ish)

## Testing

- **Unit**: client methods parse canned JSON fixtures (each endpoint's
  response shape) — no network
- **Integration**: local test server (axum stub in `tests/`) serving the
  routes used; client + output assertions, including `-o json` identity
- **E2E (manual smoke)**: against dev instance then prod (3080/4080):
  collections, search, doc roundtrip, reindex pattern, backup/restore,
  graph stats
- **Regression**: file-mode commands untouched — `cargo test -p prismsearch-cli`
  stays green; no behavioral change to legacy commands

## Out of Scope (V1)

- Cross-server copy/move (V1.1): `copy <src-coll>@<src> <dst-coll>@<dst>` —
  planned as snapshot-stream or scroll+bulk between two `PrismClient`s
- Cluster commands migration (drain/undrain/upgrade-status move into
  `prismctl cluster` API-mode group)
- `_msearch`, `_template`, ILM, aliases management, vectorize, pipelines,
  SSE, session/context endpoints, `aggregate`, `_mlt`, terms browsing
- Shell completion generation (clap_complete) — candidate for V1.1
- Progress bars for bulk import (only for backup/restore large files)

## Open items (to resolve during implementation)

1. Exact bulk endpoint shape — `POST /:collection/documents/_bulk` vs
   batched posts; match server routes
2. Backup format — `/admin/export/encrypted` requires a key; verify whether
   a plain snapshot export endpoint exists at server level, or whether
   `collection detach`/`attach` files are the practical backup artifact
3. Suggestions vs autocomplete — same endpoint (`/_suggest`) with params;
   confirm param names during implementation
