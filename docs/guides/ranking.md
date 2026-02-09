# Ranking & Boosting

Prism applies ranking adjustments after the initial search scoring to incorporate signals beyond text relevance: recency, popularity, context, field weights, and custom numeric signals.

## How ranking works

Search results go through a multi-stage scoring pipeline:

1. **Base score** — BM25 text relevance (or vector similarity, or hybrid fusion)
2. **Recency decay** — multiply score by a time-based decay factor
3. **Document boost** — multiply by per-document `_boost` value
4. **Custom signals** — add `field_value * weight` for each signal
5. **Re-sort** — results are re-sorted by adjusted score

## Schema configuration

Ranking is configured in the `boosting` section of your collection schema:

```yaml
boosting:
  recency:
    field: published_at
    decay_function: exponential
    scale: 7d
    offset: 1d
    decay_rate: 0.5

  context:
    - field: project_id
      match_current: true
      boost: 2.0

  field_weights:
    title: 2.0
    content: 1.0

  signals:
    - name: view_count
      weight: 0.001
    - name: like_count
      weight: 0.01
```

## Recency decay

Recency decay reduces scores for older documents, keeping fresh content at the top.

```yaml
boosting:
  recency:
    field: published_at          # Timestamp field
    decay_function: exponential  # exponential | linear | gauss
    scale: 7d                    # Distance at which score is multiplied by decay_rate
    offset: 1d                   # No decay within this period
    decay_rate: 0.5              # Score multiplier at scale distance
```

### Decay functions

**Exponential** (default) — smooth, continuous decay. Most natural for search:

```
score_multiplier = decay_rate ^ (distance / scale)
```

| Age | Multiplier (7d scale, 0.5 decay) |
|-----|----------------------------------|
| 0 days | 1.0 |
| 7 days | 0.5 |
| 14 days | 0.25 |
| 21 days | 0.125 |
| 30 days | 0.06 |

**Linear** — constant decrease, reaches zero:

```
score_multiplier = max(0, 1 - (1 - decay_rate) * distance / scale)
```

**Gaussian** — bell-curve decay, gentle near the center:

```
score_multiplier = exp(-0.5 * (distance / scale)^2 * ln(decay_rate) / ln(0.5))
```

### Scale and offset

- **`scale`** — the time distance at which the score is multiplied by `decay_rate`. Accepts durations: `1h`, `7d`, `30d`, `1y`, etc.
- **`offset`** — documents newer than this have no decay applied. Set to `1d` to give a 24-hour grace period for new content.

### Duration format

| Format | Meaning |
|--------|---------|
| `30s` | 30 seconds |
| `5m` | 5 minutes |
| `1h` | 1 hour |
| `7d` | 7 days |

### Example: News recency

For a news site where freshness matters a lot:

```yaml
boosting:
  recency:
    field: published_at
    decay_function: exponential
    scale: 1d
    decay_rate: 0.3
```

A 1-day-old article scores 30% of a fresh article. A 3-day-old article scores ~2.7%.

### Example: Knowledge base

For documentation where recency matters less:

```yaml
boosting:
  recency:
    field: updated_at
    decay_function: gaussian
    scale: 90d
    offset: 7d
    decay_rate: 0.8
```

Content within 7 days is not decayed. At 90 days, score is 80% of original.

## Document boost (`_boost`)

Per-document boost multipliers allow popularity-based ranking. Enable in the schema:

```yaml
system_fields:
  document_boost: true
```

Then include `_boost` when indexing documents:

```json
[
  { "id": "popular", "title": "Viral Article", "_boost": 3.0 },
  { "id": "normal", "title": "Regular Article", "_boost": 1.0 },
  { "id": "demoted", "title": "Old Post", "_boost": 0.5 }
]
```

The boost value directly multiplies the base score: a document with `_boost: 3.0` scores 3x higher than one with `_boost: 1.0`.

### Use cases for `_boost`

- **Popularity signals**: Set `_boost` proportional to view count, citation count, or engagement
- **Editorial curation**: Manually boost important content
- **Demotion**: Set `_boost < 1.0` to push down low-quality content without removing it
- **Freshness seeding**: Give newly published content a temporary high boost

## Custom ranking signals

Signals are named numeric fields that contribute additively to the score. Unlike `_boost` (which multiplies), signals add `field_value * weight`.

```yaml
boosting:
  signals:
    - name: view_count
      weight: 0.001
    - name: citation_count
      weight: 0.1
    - name: quality_score
      weight: 0.5
```

For a document with `view_count: 5000`, `citation_count: 12`, `quality_score: 0.9`:

```
signal_contribution = (5000 * 0.001) + (12 * 0.1) + (0.9 * 0.5)
                    = 5.0 + 1.2 + 0.45
                    = 6.65
```

This is added to the base score after decay and boost.

### When to use signals vs. boost

| Feature | `_boost` | `signals` |
|---------|----------|-----------|
| Effect | Multiplicative | Additive |
| Per-document | Yes | Yes (via stored fields) |
| Schema config | Just enable | Define each signal |
| Use case | Scale all relevance | Add independent score components |

Use `_boost` when you want to scale relevance (more relevant = even more boosted). Use signals when you want to add an independent score component regardless of text relevance.

## Field weights

Field weights scale the contribution of different fields during search. A weight of 2.0 on `title` means a match in the title is worth twice as much as a match with weight 1.0.

```yaml
boosting:
  field_weights:
    title: 3.0
    summary: 2.0
    content: 1.0
    tags: 1.5
```

Field weights are applied at query time in the Tantivy query parser, not during post-processing.

## Context-aware boosting

Context boosting multiplies scores for documents matching the current user's context:

```yaml
boosting:
  context:
    - field: project_id
      match_current: true
      boost: 2.0
    - field: author
      match_current: true
      boost: 1.5
```

When searching with context via the Lucene endpoint:

```json
{
  "context": { "project_id": "my-project" },
  "boosting": { "context_boost": 2.0 }
}
```

Documents where `project_id == "my-project"` get their score multiplied by 2.0. This is useful for personalization — boosting results from the user's current project, team, or session.

## Combining all ranking features

All ranking adjustments stack. The final score formula:

```
adjusted_score = (base_score * recency_decay * document_boost)
               + sum(signal_value * signal_weight)
```

Example with all features:

```yaml
system_fields:
  indexed_at: true
  document_boost: true

boosting:
  recency:
    field: _indexed_at
    decay_function: exponential
    scale: 14d
    decay_rate: 0.5

  field_weights:
    title: 2.0
    content: 1.0

  signals:
    - name: popularity
      weight: 0.01
```

For a document with base score 5.0, indexed 7 days ago, `_boost: 1.5`, `popularity: 200`:

```
recency_decay = 0.5^(7/14) = 0.707
adjusted = (5.0 * 0.707 * 1.5) + (200 * 0.01)
         = 5.30 + 2.0
         = 7.30
```

## Two-Phase Ranking (Re-ranking)

For complex ranking models that are too slow to run on the full corpus, Prism supports two-phase ranking:

1. **Phase 1** — Cheap retrieval (BM25/vector) fetches a large candidate set
2. **Phase 2** — An expensive re-ranker scores only the top candidates

This is configured at the schema level and can be overridden per-request.

### Schema configuration

Add a `reranking` section to your collection schema:

```yaml
reranking:
  type: cross_encoder
  candidates: 100
  text_fields:
    - title
    - content
  cross_encoder:
    model_id: "cross-encoder/ms-marco-MiniLM-L-6-v2"
    max_length: 512
```

| Parameter | Default | Description |
|-----------|---------|-------------|
| `type` | required | `cross_encoder` or `score_function` |
| `candidates` | `100` | Number of Phase 1 candidates to retrieve |
| `text_fields` | `[]` | Document fields to extract text from for re-ranking |

### Cross-encoder re-ranking

Cross-encoder models score (query, document) pairs using a transformer model. They produce more accurate relevance scores than BM25 or bi-encoder similarity, but are too slow for full corpus search.

```yaml
reranking:
  type: cross_encoder
  candidates: 100
  text_fields:
    - title
    - content
  cross_encoder:
    model_id: "cross-encoder/ms-marco-MiniLM-L-6-v2"
    max_length: 512
```

| Parameter | Default | Description |
|-----------|---------|-------------|
| `model_id` | `cross-encoder/ms-marco-MiniLM-L-6-v2` | HuggingFace model ID |
| `model_path` | auto-downloaded | Local path to model files |
| `max_length` | `512` | Maximum token length for input pairs |

The model is auto-downloaded from HuggingFace on first use. Requires the `provider-onnx` feature.

### Score function re-ranking

Score functions let you re-rank using arithmetic expressions that reference the original score and document fields:

```yaml
reranking:
  type: score_function
  score_function: "_score * popularity * 0.01"
```

Supported operations:
- `_score` — the original search score
- Field names — any numeric field in the document
- Arithmetic: `+`, `-`, `*`, `/`
- `log(x)` — natural logarithm
- Parentheses for grouping

Examples:
```
_score * 2                          # Double all scores
_score + log(likes + 1)            # Boost by engagement
_score * (1 + popularity * 0.001)  # Mild popularity boost
```

### Per-request override

Override reranking at search time via the `rerank` field:

```json
{
  "query": "machine learning",
  "rerank": {
    "enabled": true,
    "candidates": 50,
    "text_fields": ["title", "content"]
  }
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Enable/disable reranking for this request |
| `candidates` | schema default | Override Phase 1 candidate count |
| `text_fields` | schema default | Override text fields for re-ranking |

Set `"enabled": false` to skip reranking for a specific request even when the collection has it configured.

### How it works

```
Query → Phase 1: Retrieve `candidates` docs (BM25/vector)
      → Phase 2: Re-rank with expensive model
      → Return top `limit` results
```

1. The search limit is temporarily expanded to `candidates` (default 100)
2. Phase 1 retrieves candidates using the configured backends (text, vector, or hybrid)
3. Phase 2 scores each candidate with the re-ranker
4. Results are re-sorted by the new scores and truncated to the original `limit`
5. If re-ranking fails, original results are returned (graceful degradation)
