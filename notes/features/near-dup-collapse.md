# Semantic near-duplicate collapse

Issue #3. Drop results whose embedding is too similar to a higher-scoring
result already kept (greedy diversification), so a page isn't dominated by many
rephrasings of the same content. Sibling of exact field collapse (#2).

## Status
- [x] Pure core: `ranking::near_dup::collapse_near_duplicates` (greedy cosine
      dedup over an `id -> vector` map) + `NearDupConfig`. Unit-tested.
- [x] Vector plumbing: `SearchBackend::get_vectors` (default empty),
      `VectorBackend`/`HybridBackend` impls, `CollectionManager::vectors_for`.
      Integration-tested (`test_get_vectors_returns_stored_embeddings`).
- [x] API wiring: `near_dup` on `SearchRequest`, applied in the search route
      after field collapse. E2e-tested (`test_search_near_duplicate_collapse`).

## Design decisions
- **Vector source (chosen):** fetched from the vector backend for the returned
  result ids, not carried on `SearchResult`. `SearchResult` gains no serialized
  field; the route builds an `id -> vector` map via `manager.vectors_for` and
  passes it to the pure core. `get_vector` reads the stored HNSW point in the
  segment (`HnswBackend::get_point`).
- **API shape:** `"near_dup": { "threshold": 0.95 }`. `threshold` = cosine
  similarity at/above which two hits are duplicates (default 0.95).
- **Order:** greedy over score-ordered results; the highest-scoring member of
  each near-duplicate cluster survives. Compared against *all* kept vectors, not
  just the previous one.
- **Missing vectors:** hits with no vector in the map are always kept (cannot be
  compared) — e.g. text-only hits in a hybrid result, or a collection with no
  vector backend (`vectors_for` returns empty → no-op).

## Known limitations
- Runs on the already-paginated page (post-`limit`), like `min_score` /
  `collapse`, so a collapsed page may contain fewer than `limit` hits.
- Only the `/collections/:c/search` route is wired (not `multi_index_search`),
  matching the field-collapse decision.
- Greedy O(kept²) cosine over the page; fine for page-sized result sets.
