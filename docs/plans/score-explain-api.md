# Plan: Score explanation API (Prism's part of debug view)

**Status:** Draft for review
**Goal:** Let a search product render a "why did this result rank here?" debug
view. Prism's responsibility is narrow: expose a structured breakdown of how
**Prism's own ranking pipeline** produced each result's final score. The
product layer handles everything else (provenance across sources, cache layer,
network classification) — Prism doesn't know those things.

This is the "score-explain" half of the debug view. It is cheap because the
components already exist as intermediate values inside the pipeline; we keep
them as named values on a debug flag instead of discarding them.

---

## Scope: what Prism explains (and what it does not)

| Debug-view component | Owner | Prism's role |
|---|---|---|
| **Score-explain** (base, recency, boost, signals, rerank) | **Prism** | Produces the breakdown |
| Provenance (Prism/Exa/Brave/Ingested) | Product | Prism only sees its own docs |
| Cache layer / fetch path | Product | Prism has no notion |
| Network (tor/clearnet/i2p) | Crawler writes it; product classifies | Prism returns the stored field, doesn't compute |
| Personalization blend (per earlier `personalization.md`) | Product | Prism's score is the "neutral" input |

So the deliverable is: **per-result structured score breakdown, opt-in via a
request flag, surfaced on the existing search response.**

---

## Prism's scoring pipeline (the thing we explain)

Verified against source. Five stages, two call sites:

```
Query
  → Backend (BM25 / vector / hybrid)          [backends/text.rs]
      stage 1: BASE score (incl. field boost at query time)
  → apply_ranking_adjustments()               [ranking/mod.rs:210, called at text.rs:1063]
      stage 2: RECENCY decay   (multiply)
      stage 3: DOC BOOST        (multiply by _boost)
      stage 4: CUSTOM SIGNALS   (add Σ field_value × weight)
  → Reranker Phase 2 (optional)               [collection/manager.rs:452]
      stage 5: RERANK (cross-encoder OR score_function) — replaces score
```

Each stage is already computed as an intermediate `f64` before being folded
into `adjusted_score`. Today we throw the intermediates away. The change is to
**keep them in a `Vec` when a debug flag is set.**

The two reranker types share one trait (`Reranker`):
- `CrossEncoderReranker` — opaque model score, replaces base.
- `ScoreFunctionReranker` — arithmetic expression over `_score` and fields
  (e.g. `_score * (1 + popularity)`). This one is itself decomposable and
  should explain its evaluated sub-terms when feasible.

---

## Data model

New types in `prism/src/ranking/mod.rs`:

```rust
/// How a score component contributes to the running total.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreOp {
    /// The starting point. Exactly one per explanation.
    Base { raw: f64 },
    /// Multiply the running score by `factor`.
    Multiply { factor: f64, result: f64 },
    /// Add `delta` to the running score.
    Add { delta: f64, result: f64 },
    /// Replace the running score with `value` (Phase 2 reranker).
    Replace { value: f64, previous: f64 },
}

/// One named step in the score pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreComponent {
    /// Human-readable stage name: "base", "recency_decay", "doc_boost",
    /// "signal:view_count", "rerank:cross_encoder", "rerank:score_function", …
    pub name: String,
    /// What this component did to the score.
    #[serde(flatten)]
    pub op: ScoreOp,
    /// Optional human note ("exponential, 7d scale, 0.5 rate", "field missing → skipped").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Full breakdown for one result, evaluable top-to-bottom.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreExplanation {
    pub components: Vec<ScoreComponent>,
    /// The score after the last component applied. Equals SearchResult.score.
    pub final_score: f64,
}
```

Surface on the search result:

```rust
// backends/trait.rs
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, Value>,
    pub highlight: Option<HashMap<String, Vec<String>>>,
    /// Present only when the request asked for an explanation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_explanation: Option<ScoreExplanation>,
}
```

Wire-level request flag — keep the name generic so the product's `?debug=1`
maps cleanly:

```rust
// api/routes.rs — SearchRequest (and SimpleSearchRequest, MultiSearchRequest)
/// Return per-result score breakdowns. Off by default.
#[serde(default)]
pub explain: bool,
```

And thread it down to the backend via the existing `Query`:

```rust
// backends/trait.rs — Query
pub explain: bool,   // default false
```

---

## Implementation plan

### Files to change

| File | Change |
|---|---|
| `prism/src/ranking/mod.rs` | `ScoreOp`, `ScoreComponent`, `ScoreExplanation`; `apply_ranking_adjustments` gains an `explain: bool` param and populates an explanation when set |
| `prism/src/backends/trait.rs` | `Query.explain`; `SearchResult.score_explanation` field |
| `prism/src/backends/text.rs` | Pass `query.explain` into `apply_ranking_adjustments`; attach explanation to result |
| `prism/src/collection/manager.rs` | At the Phase 2 rerank call site (~line 452), append a `Replace` component when `explain` is set; for `ScoreFunctionReranker`, pull sub-term breakdown if available |
| `prism/src/api/routes.rs` | `explain` on the three search request structs; forward to `Query` |
| `prism/src/ranking/score_function.rs` | (Nice-to-have) expose evaluated sub-terms so `score_function` rerank can list them |
| `prism/docs/guides/ranking.md` | Document `explain: true` and the response shape |
| `prism/docs/guides/personalization.md` | One paragraph: the product's debug view combines Prism's `score_explanation` (neutral pipeline) with the product's own provenance + blend |

### Step-by-step

**Step 1 — Types.** Add `ScoreOp`, `ScoreComponent`, `ScoreExplanation` to
`ranking/mod.rs`. Add `explain: bool` to `Query` and `SearchRequest` family.
Add `score_explanation: Option<ScoreExplanation>` to `SearchResult`.

**Step 2 — Populate in `apply_ranking_adjustments`.** Today the loop mutates
`result.adjusted_score` in place. Branch on `explain`:

```rust
pub fn apply_ranking_adjustments(
    results: &mut [RankableResult],
    config: &RankingConfig,
    now: SystemTime,
    explain: bool,                     // NEW
) {
    for result in results.iter_mut() {
        let mut score = result.score as f64;
        let mut comps: Vec<ScoreComponent> = if explain {
            vec![ScoreComponent {
                name: "base".into(),
                op: ScoreOp::Base { raw: score },
                note: None,
            }]
        } else { vec![] };

        if let Some(dc) = &config.recency_decay {
            if let Some(ts) = result.indexed_at_micros {
                let factor = compute_decay_from_micros(dc, ts, now);
                let result = score * factor;
                if explain { comps.push(ScoreComponent {
                    name: "recency_decay".into(),
                    op: ScoreOp::Multiply { factor, result },
                    note: Some(format!("{:?}, scale={}s", dc.function, dc.scale.as_secs())),
                }); }
                score = result;
            } else if explain {
                comps.push(ScoreComponent { name: "recency_decay".into(),
                    op: ScoreOp::Multiply { factor: 1.0, result: score },
                    note: Some("no _indexed_at → skipped".into()) });
            }
        }
        // … same pattern for doc_boost (Multiply) and each signal (Add, name="signal:<field>")
        result.adjusted_score = score as f32;
        if explain { result.explanation = Some(ScoreExplanation { components: comps, final_score: score }); }
    }
    // sort unchanged
}
```

The non-explain path is identical to today (just an extra `if explain` check
per stage — branch predicted away). Zero overhead on the hot path.

**Step 3 — Carry explanation from `RankableResult` to `SearchResult`.** Add
`explanation: Option<ScoreExplanation>` to `RankableResult`; the text backend
maps it onto `SearchResult.score_explanation` when building the response.

**Step 4 — Phase 2 rerank.** In `collection/manager.rs` at the rerank call
site, when `query.explain` and a reranker ran, append one component to each
result's explanation:

```rust
ScoreComponent {
    name: format!("rerank:{}", reranker.name()),  // "rerank:cross_encoder" etc.
    op: ScoreOp::Replace { value: new_score, previous: old_score },
    note: None,
}
```

For `ScoreFunctionReranker` specifically, if a sub-term breakdown is cheaply
available (Step 6), include them as additional `Add`/`Multiply` components
named `score_function:<term>` instead of a single opaque `Replace`.

**Step 5 — API + docs.** Add `explain` to request structs, forward it, and
document the response shape. Example response:

```json
{
  "id": "doc-123",
  "score": 7.30,
  "fields": { "title": "…", "_boost": 1.5, "view_count": 200 },
  "score_explanation": {
    "final_score": 7.30,
    "components": [
      { "name": "base", "type": "base", "raw": 5.0 },
      { "name": "recency_decay", "type": "multiply", "factor": 0.707, "result": 3.535,
        "note": "Exponential, scale=1209600s" },
      { "name": "doc_boost", "type": "multiply", "factor": 1.5, "result": 5.30 },
      { "name": "signal:view_count", "type": "add", "delta": 2.0, "result": 7.30,
        "note": "view_count=200 × weight=0.01" }
    ]
  }
}
```

**Step 6 — (Optional, defer) `score_function` sub-term breakdown.** The
expression evaluator in `score_function.rs` evaluates an AST. Walking it to
emit evaluated sub-terms is a nice-to-have that makes ad-hoc `score_function`
queries transparent in the debug view. Not required for the MVP — a single
`Replace` with the expression string as `note` is acceptable for v1.

### Tests

- Unit (`ranking/mod.rs`): `apply_ranking_adjustments` with `explain=true`
  produces correct components for each stage combination; `explain=false`
  leaves `explanation` None and matches old behavior exactly (golden test).
- Unit: missing `_indexed_at` → recency component present with skip note.
- Integration (`prism/tests/`): `POST /collections/c/search` with
  `"explain": true` returns `score_explanation` whose components re-sum to
  `final_score`; without the flag, the field is absent.
- Integration: with a `score_function` configured, the explanation names the
  reranker and records the replace.

### Open questions for review

1. **Naming**: `explain` (Lucene/ES parlance) vs `debug` (matches the product's
   `?debug=1`). I lean `explain` in Prism's API since it's precise; the product
   maps `debug=1 → explain=true`.
2. **`score` field type**: keep `f32` on the wire (status quo) or widen the
   explanation values to `f64` for precision in the breakdown? I propose `f64`
   inside `ScoreExplanation` (full precision for debugging) while `score`
   stays `f32`.
3. **Defer Step 6?** I recommend shipping Steps 1–5 first; `score_function`
   sub-terms can land in a follow-up without an API change.

### Non-goals

- Provenance, cache-layer, or network-classification data (product's job).
- Personalization-blend breakdown (product's job — Prism's breakdown is the
  "neutral" leg the product blends from).
- Explaining *why* BM25 scored a term how it did (Tantivy internals). We
  expose the backend's output score as the opaque `base`; term-level TF/IDF is
  out of scope.
- Enabling explain on aggregations or suggestions.

### Estimated size

~180 lines of new types + instrumentation, ~150 lines of tests. No new crate
or dependency. Off by default; the only runtime cost is a per-stage branch
when enabled.
