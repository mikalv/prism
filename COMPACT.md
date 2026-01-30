# Ranking/Relevance Scoring Implementation - Session Compact

**Date:** 2026-01-30
**Issue:** [#26](https://github.com/mikalv/prism/issues/26)

## Goal

Improve search result quality through better ranking and relevance scoring. MVP focuses on field boosting, recency decay, and popularity signals.

## Current State (Storage Prep Done)

### Schema Infrastructure (✅ Complete)

**SystemFieldsConfig** in `CollectionSchema`:
```yaml
system_fields:
  indexed_at: true    # Auto-timestamp on index (default: enabled)
  document_boost: false  # Per-doc _boost field (default: disabled)
```

**BoostingConfig** in `CollectionSchema`:
```yaml
boosting:
  recency:
    field: _indexed_at
    decay_function: exponential  # exponential | linear | gauss
    scale: 7d
    decay_rate: 0.5
  field_weights:
    title: 2.0
    content: 1.0
  context:
    - field: project_id
      match_current: true
      boost: 1.5
```

**HybridConfig** in `CollectionSchema`:
```yaml
hybrid:
  default_strategy: rrf  # rrf | weighted
  rrf_k: 60
  text_weight: 0.5
  vector_weight: 0.5
```

**BM25 Config** in `TextBackendConfig` (reserved, Tantivy hardcoded):
```yaml
backends:
  text:
    fields: [...]
    bm25_k1: 1.2  # Not applied yet - Tantivy limitation
    bm25_b: 0.75  # Not applied yet - Tantivy limitation
```

### What's Already Working

1. **`_indexed_at` field** - Auto-injected on every document index
2. **`_boost` field** - Stored if `document_boost: true`
3. **RRF merge** - `HybridSearchCoordinator::merge_rrf_public()`
4. **Weighted merge** - `HybridSearchCoordinator::merge_weighted_public()`
5. **Schema parsing** - All config structures deserialize correctly

## MVP Implementation Plan (~6 hours)

### Task 1: Field Boosting (~2h)

Apply `field_weights` from `BoostingConfig` when building Tantivy queries.

**File:** `prism/src/backends/text.rs`

**Current:** QueryParser searches all fields equally
```rust
let query_parser = QueryParser::for_index(&coll.index, fields_to_search);
```

**Target:** Apply boost per field
```rust
// In search() method
let boosting = schema.boosting.as_ref();
let field_weights = boosting.map(|b| &b.field_weights);

// Option A: Use Tantivy's BoostQuery
// Option B: Post-process scores with field weights
```

**Key files:**
- `prism/src/backends/text.rs` - `search()` method (~line 316)
- `prism/src/collection/manager.rs` - Pass schema to search

### Task 2: Recency Decay (~3h)

Apply time-decay function to `_indexed_at` in search results.

**File:** `prism/src/backends/text.rs` or new `prism/src/ranking/mod.rs`

**Decay functions:**
```rust
fn exponential_decay(age_days: f64, scale_days: f64, decay_rate: f64) -> f64 {
    decay_rate.powf(age_days / scale_days)
}

fn linear_decay(age_days: f64, scale_days: f64) -> f64 {
    (1.0 - age_days / scale_days).max(0.0)
}

fn gaussian_decay(age_days: f64, scale_days: f64, decay_rate: f64) -> f64 {
    (-0.5 * (age_days / scale_days).powi(2) * decay_rate.ln().abs()).exp()
}
```

**Integration point:**
```rust
// After getting search results, before returning
for result in &mut results {
    if let Some(recency) = &boosting.recency {
        let indexed_at = result.fields.get("_indexed_at");
        let decay = compute_decay(indexed_at, now, recency);
        result.score *= decay;
    }
}
// Re-sort by new scores
results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
```

### Task 3: Popularity Boost (~1h)

Multiply score by `_boost` field value.

**Integration point:**
```rust
// After recency decay
for result in &mut results {
    if let Some(boost) = result.fields.get("_boost").and_then(|v| v.as_f64()) {
        result.score *= boost;
    }
}
```

## Files to Modify

```
prism/src/backends/text.rs           # Field boosting + score adjustments
prism/src/collection/manager.rs      # Pass schema/boosting config to backends
prism/src/ranking/mod.rs             # NEW: Decay functions, score adjustments
prism/src/ranking/decay.rs           # NEW: Recency decay implementations
prism/src/lib.rs                     # Add ranking module
```

## Test Cases

```rust
#[test]
fn test_field_boosting() {
    // Doc with match in "title" should score higher than "content"
}

#[test]
fn test_recency_decay_exponential() {
    // 7-day-old doc with scale=7d, decay=0.5 should have 0.5x multiplier
}

#[test]
fn test_popularity_boost() {
    // Doc with _boost=2.0 should score 2x higher than _boost=1.0
}

#[test]
fn test_combined_ranking() {
    // Field boost + recency + popularity all applied
}
```

## Schema Example

```yaml
collection: articles
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
        stored: true
      - name: content
        type: text
        indexed: true
system_fields:
  indexed_at: true
  document_boost: true
boosting:
  recency:
    field: _indexed_at
    decay_function: exponential
    scale: 30d
    decay_rate: 0.5
  field_weights:
    title: 3.0
    content: 1.0
```

## What's NOT in MVP

- **BM25 tuning** - Tantivy hardcodes k1=1.2, b=0.75
- **Custom scoring functions** - User-defined expressions (complex)
- **Context boosting** - Match current session/project (needs context API)
- **Learned weights** - ML-based weight optimization

## Related Commits

- `53596c5` - feat(schema): Add ranking system fields for Issue #26 forward compatibility
- Storage infrastructure ready, scoring logic needs implementation

## Success Criteria

- [x] Field weights applied in text search (title boosted over content)
- [x] Recency decay reduces scores for older documents
- [x] `_boost` field multiplies document scores
- [x] Results re-sorted after score adjustments
- [x] Tests for each ranking feature (5 integration tests)
- [ ] Works with hybrid search (text + vector) - text ranking done, hybrid coordinator needs update

## Implementation Summary

**New files:**
- `prism/src/ranking/mod.rs` - Main ranking module with `apply_ranking_adjustments()`
- `prism/src/ranking/decay.rs` - Decay functions (exponential, linear, gaussian)
- `prism/tests/ranking_test.rs` - 5 integration tests

**Modified files:**
- `prism/src/lib.rs` - Added `ranking` module
- `prism/src/schema/mod.rs` - Exported `BoostingConfig`, `RecencyDecayConfig`
- `prism/src/backends/text.rs` - Added field boosting at query time, recency/popularity post-processing
