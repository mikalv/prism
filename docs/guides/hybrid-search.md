# Hybrid Search

Hybrid search combines full-text BM25 scoring with vector similarity search to get the best of both worlds: keyword precision and semantic understanding.

## When to use hybrid search

| Search type | Strengths | Weaknesses |
|-------------|-----------|------------|
| Text only | Exact keyword matching, acronyms, proper nouns | Misses synonyms and semantic meaning |
| Vector only | Understands meaning, handles paraphrases | Can miss exact terms, poor with rare words |
| **Hybrid** | **Both keyword precision and semantic recall** | Slightly more complex to tune |

Hybrid search is recommended for most use cases, especially RAG (Retrieval Augmented Generation) applications.

## Setup

A collection needs both `text` and `vector` backends configured:

```yaml
collection: knowledge_base
backends:
  text:
    fields:
      - name: id
        type: string
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true
  vector:
    embedding_field: content_vector
    dimension: 384
    distance: cosine

embedding_generation:
  enabled: true
  model: nomic-embed-text
  source_field: content
  target_field: content_vector

hybrid:
  default_strategy: rrf
  rrf_k: 60
  text_weight: 0.5
  vector_weight: 0.5
```

## Searching

When both backends exist, a text query automatically runs hybrid search:

```bash
curl -X POST http://localhost:3080/collections/knowledge_base/search \
  -d '{ "query": "how to handle errors in async rust" }'
```

This runs the query against both:
1. **Text backend** — BM25 keyword search for "handle errors async rust"
2. **Vector backend** — semantic similarity search using the query's embedding

Results from both are merged using the configured strategy.

### Override strategy per query

```json
{
  "query": "error handling patterns",
  "merge_strategy": "weighted",
  "text_weight": 0.3,
  "vector_weight": 0.7
}
```

## Merge strategies

### Reciprocal Rank Fusion (RRF) — default

RRF combines results based on their rank position, not raw scores. This makes it robust when text and vector scores are on different scales.

```
rrf_score(doc) = 1/(k + rank_text) + 1/(k + rank_vector)
```

Where `k` is a smoothing parameter (default: 60).

```yaml
hybrid:
  default_strategy: rrf
  rrf_k: 60
```

**When to use RRF:**
- You don't know the relative quality of text vs. vector results
- Score distributions differ significantly between backends
- You want a robust, parameter-light approach

**Tuning `rrf_k`:**
| Value | Effect |
|-------|--------|
| 1 | Top-ranked results dominate heavily |
| 60 | Default — balanced influence across ranks |
| 100+ | Flatter ranking, more equal weight to all positions |

### Weighted merge

Weighted merge normalizes scores from each backend and combines them with explicit weights:

```
final_score = text_weight * norm_text_score + vector_weight * norm_vector_score
```

```yaml
hybrid:
  default_strategy: weighted
  text_weight: 0.6
  vector_weight: 0.4
```

**When to use weighted merge:**
- You know the relative importance of keyword vs. semantic matching
- You want fine-grained control over the blend
- Your score distributions are reasonably comparable

**Weight guidelines:**

| Use case | text_weight | vector_weight |
|----------|-------------|---------------|
| Keyword-heavy (code search, exact terms) | 0.7 | 0.3 |
| Balanced (general documents) | 0.5 | 0.5 |
| Semantic-heavy (Q&A, natural language) | 0.3 | 0.7 |
| Mostly semantic (chat, recommendations) | 0.2 | 0.8 |

## Explicit vector queries

You can provide a pre-computed vector alongside a text query:

```json
{
  "query": "error handling",
  "vector": [0.12, -0.05, 0.34, ...],
  "merge_strategy": "rrf"
}
```

This is useful when:
- You want to embed the query with a different model than the one configured
- You want to search with a vector derived from an image or other modality
- You want to cache query embeddings client-side

## Schema defaults vs. per-query overrides

The `hybrid` section in the schema sets defaults:

```yaml
hybrid:
  default_strategy: rrf
  rrf_k: 60
  text_weight: 0.5
  vector_weight: 0.5
```

These can be overridden on every search request:

```json
{
  "query": "...",
  "merge_strategy": "weighted",
  "text_weight": 0.8,
  "vector_weight": 0.2
}
```

## Tuning tips

### Start with RRF

RRF is robust and requires no weight tuning. Start here and only switch to weighted merge if you have specific requirements.

### Use the Lucene endpoint for complex hybrid queries

The Lucene endpoint supports hybrid search with boosting, facets, and context:

```json
{
  "collection": "knowledge_base",
  "query": "async error handling",
  "merge_strategy": "rrf",
  "boosting": {
    "recency_enabled": true,
    "recency_field": "updated_at",
    "recency_decay_days": 30
  },
  "facets": [
    { "field": "language", "agg_type": "terms", "size": 5 }
  ]
}
```

### Monitor with benchmarks

Use the CLI benchmark tool to compare strategies:

```bash
# Prepare a queries file (one query per line)
echo "error handling in rust" > queries.txt
echo "async programming patterns" >> queries.txt
echo "tokio runtime" >> queries.txt

# Benchmark
prism-cli benchmark -c knowledge_base -q queries.txt -r 20
```

## Vector weight in the schema

The `vector_weight` field on the vector backend config sets the default weight for the vector component in hybrid search:

```yaml
backends:
  vector:
    embedding_field: content_vector
    dimension: 384
    vector_weight: 0.5    # Default weight in hybrid merge
```

This is the same as `hybrid.vector_weight` but set at the backend level. The `hybrid` section takes precedence if both are set.
