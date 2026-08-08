# TODOs — Bug Audit 2026-07-30 (v0.6.10, main @ 7bfa824)

Findings from a full-codebase bug hunt. Ordered by severity. File references point at
the code as of commit `7bfa824`.

Recommended attack order: **C2 + C1 first** (active data loss), then **C4 + C5**
(security), then **C3** (the product's core promise), then the rest.

---

## 🔴 Critical

### C1. Vector persistence drops buffered vectors (silent data loss)
- [x] `prism/src/backends/vector/index.rs:179` — `add()` only rebuilds the HNSW graph
      every `rebuild_threshold` (32) inserts; newer points live only in the
      `points`/`keys` buffer.
- [x] `save()` (v2 path, ~line 226) serializes **only the built graph** — the buffered
      tail (up to 31 newest vectors) is lost on save/restart. The doc ids remain in the
      segment's `documents` map, so `get()`/stats see them but vector search never
      returns them again.
- Fix ideas: force `rebuild()` before save when `built_size < keys.len()`, or persist
  the buffered points alongside the graph (v3 format). Add a regression test:
  index 50 docs → save → load → search finds all 50.

### C2. Non-atomic writes can wipe an entire collection index
- [x] `prism-storage/src/local.rs:56` — `write()` is a raw `fs::write` (no tmp file +
      rename, no fsync). The whole vector index for a collection is **one JSON file
      rewritten on every batch**; a crash mid-write leaves a truncated file →
      startup logs "rebuilding" and starts with an **empty index**.
- Fix: write to `path.tmp` + `sync_all` + atomic `rename`. Consider the same in
  `write_sync` (line 261).

### C3. Hybrid search is dead code — text+vector merge never happens over HTTP
- [x] `prism/src/collection/manager.rs:842` — `hybrid_search()` and `embed_query()`
      (line 621) have **no callers** in any API route.
- [x] `prism/src/api/routes.rs:160` — when the client sends `vector`, the text query is
      **discarded** (`qstr` = vector OR text, never both). The coordinator's vector
      branch then runs the text side with `""`.
- [x] Text queries against hybrid collections are never auto-embedded, despite
      `embedding_generation.enabled` and the documented
      `{"query": "...", "merge_strategy": "rrf"}` usage.
- Fix: in the search route, when the collection is hybrid and no `vector` is given,
  embed the query text (`embed_query`) and pass BOTH text + vector into the hybrid
  path; when `vector` is given, keep the text query too.

### C4. ACL bypass: cross-collection reads with a scoped key
- [x] `prism/src/security/middleware.rs:57` (`is_authorized`) — only
      `/collections/:c/...` and admin paths are permission-checked; everything else is
      "authentication alone suffices". `/api/search` (which now searches **all**
      collections), `/_msearch`, and `/:collections/_search` bypass collection ACLs
      entirely — a key with read on collection A can read collection B.
- Fix: resolve target collections in those handlers and check per-collection Search
  permission (filter out unauthorized collections in the "All" case).

### C5. Unauthenticated `/admin/tasks` + `/stats/load`, both returning mock data
- [x] `prism/src/security/middleware.rs:13` — both routes are in `AUTH_WHITELIST`
      (fully public, including everything under `/admin/tasks/...` — a landmine when a
      real task-cancel endpoint appears). A public `/admin/*` path also undermines the
      admin-gating convention.
- [x] `prism/src/api/routes.rs:1134` (`get_load_stats`) and `:1159` (`get_tasks`) —
      hardcoded mock data (CPU 12.5%, "Index compaction 45%") that the DevTools UI
      renders as real.
- Fix: implement real system metrics (e.g. `sysinfo`) and a real task registry, or
  return `501`/empty until then. Remove both from the whitelist (or move load stats
  under `/stats/` semantics with auth).

---

## 🟠 High

### H1. Compaction never runs — tombstones are never reclaimed
- [x] `prism/src/backends/vector/shard.rs:159` — `seal_active()` is only called from
      tests. Production shards never get sealed segments, so `compact_shard`
      (invoked in `delete()`) never finds candidates; the active segment grows forever.
- Fix: seal on a size/count threshold during `index()`, then compaction becomes live.

### H2. Lost-write race in vector/graph persistence
- [x] `prism/src/backends/vector/backend.rs` `index()`/`delete()` and
      `prism/src/backends/graph/shard.rs` `persist()` — snapshot is serialized under
      the lock but written **after** the lock is released; two concurrent calls can
      land on disk out of order (older snapshot overwrites newer).
- Fix: sequence saves per collection (e.g. an async mutex around save, or a
  monotonically-versioned write that refuses to go backwards).

### H3. Shard routing uses std `DefaultHasher` (unstable across Rust releases)
- [ ] `prism/src/backends/vector/shard.rs:268` (`shard_for_doc`, also used by graph) —
      hash output is not guaranteed stable across Rust versions; a toolchain upgrade
      can silently re-route persisted docs to the wrong shard (get/delete miss them).
- Fix: switch to an explicitly stable hash (xxhash/fnv/siphash with fixed keys) with a
  format-version bump + migration.

### H4. Tombstones shrink result sets below k
- [ ] `prism/src/backends/vector/segment.rs:106` — `search()` fetches k from HNSW and
      filters tombstones **afterwards**; segments with many deletions return fewer
      than k hits even when enough live docs exist.
- Fix: oversample by the tombstone ratio (`k * total/live`, capped).

### H5. Async index queue: failures are silently dropped after 202
- [ ] `prism/src/api/server.rs:486` — worker logs errors and moves on; the client
      already received 202 with `indexed: N`. No retry, no dead-letter, no job status.
- Fix: minimal viable = in-memory failed-jobs list + `/admin/tasks` (real one) to
  inspect; better = retry with backoff + DLQ.

### H6. Indexing into graph-only collections silently drops documents
- [ ] `prism/src/collection/manager.rs:380-393` — no backend → `Ok(())` → API returns
      201 while every doc is discarded.
- Fix: return 400 ("collection has no document backend") or actually build graph
  nodes/edges from configured `from_field`/`to_field`.

### H7. Hybrid pagination is incoherent
- [ ] `prism/src/backends/hybrid.rs:290` (`search()`) — sub-queries apply `offset` per
      backend, then the merge only truncates to `limit` (RRF computed on page-2
      slices; merged output never skips). Page 2 is not a continuation of page 1.
- Fix: fetch `offset+limit` from each backend with offset 0, merge, then skip/take.

### H8. Graph `scope=Global` gives silently wrong traversals
- [ ] `prism/src/backends/graph/backend.rs:125-145` — BFS/shortest_path are
      shard-local: cross-shard edges are never traversed (BFS) and `shortest_path`
      returns `None` for endpoints on different shards even when a path exists.
- [ ] `remove_node` only cleans inbound edges within the node's own shard — dangling
      inbound edges remain in other shards under Global scope.
- Fix: either implement cross-shard traversal (frontier exchange), or restrict
  `scope=Global` until it exists and document the limitation loudly.

---

## 🟡 Medium / Low

- [x] `prism/src/backends/text.rs:2515` — `more_like_this` still reports
      `total = results.len()` (page size), same bug class fixed in `search()`.
- [x] `prism/src/security/middleware.rs:81` — permission heuristic
      `path.contains("/search")`: a collection literally named `search` makes document
      POSTs require only the Search permission.
- [ ] Write amplification: every batch persists the **entire** index as JSON
      (`hnsw_data: Vec<u8>` becomes a JSON number array ≈ 4× size); graph persists the
      whole graph on every `add_node`/`add_edge` → O(N²) for bulk imports.
      Move to incremental/per-segment persistence + binary format.
- [ ] `prism/src/backends/vector/index.rs:188` — `ef_search` parameter is ignored
      (`_ef_search`); searches always use build-time defaults. *(Upstream limitation: `instant-distance 0.6.1` hardcodes `ef_search` at index build time and overwrites any per-query parameter).*
- [x] `prism/src/backends/vector/backend.rs:152` — `initialize()` detects dimension
      mismatch vs persisted index but not a changed distance metric.
- [x] `prism-storage/src/encrypted.rs` — no AAD binding ciphertext to its path
      (encrypted-file swap goes undetected); `head()` reports encrypted size.
- [x] `prism/src/backends/graph/shard.rs:169` — `add_edge` does not dedupe identical
      edges; Dijkstra treats NaN weights as `Ordering::Equal`.
- [x] `prism/src/collection/manager.rs:1210` — `multi_search` swallows per-collection
      errors (only logs); response should flag failed collections.
- [x] `prism/src/api/routes.rs:218` — search metrics always label
      `search_type => "text"` regardless of actual search type.

---

## ✅ Verified fixed in 0.6.10 (from the previous audit)

- Vector search offset pagination; exact aggregations beyond 10k (`6aadbb5`).
- `/_admin/*` requires Admin permission (`a8128b1`); collection-name validation /
  path-traversal guard in `delete_collection_data` (`a80493e`).
- `DELETE /collections/:name` purges on-disk data (no resurrection) (`4590e3a`).
- Distance metric now persisted per shard/segment (sharded format).
- Embedding failure during indexing returns an error instead of silently indexing
  text-only.
- Collector-level fast-field sort; ES-compat error mapping to 400; `exists` queries;
  `_source` filtering with wildcards; `X-Elastic-Product` header.
