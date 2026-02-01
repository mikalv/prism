# More Like This

The More Like This (MLT) API finds documents similar to a given document or text. It works by extracting significant terms from the source and using them as a search query.

## Endpoint

```
POST /collections/:collection/_mlt
```

## Find documents similar to an existing document

```bash
curl -X POST http://localhost:3080/collections/articles/_mlt \
  -H "Content-Type: application/json" \
  -d '{
    "like": { "_id": "article-42" },
    "fields": ["title", "content"],
    "size": 5
  }'
```

## Find documents similar to arbitrary text

```bash
curl -X POST http://localhost:3080/collections/articles/_mlt \
  -H "Content-Type: application/json" \
  -d '{
    "like_text": "Rust async programming with tokio for high-performance servers",
    "fields": ["content"],
    "size": 10
  }'
```

## Request fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `like` | object | null | Source document: `{ "_id": "doc-id" }` |
| `like_text` | string | null | Source text (alternative to `like`) |
| `fields` | string[] | all text fields | Fields to extract terms from |
| `min_term_freq` | integer | 2 | Minimum times a term must appear in the source |
| `min_doc_freq` | integer | 5 | Minimum documents a term must appear in |
| `max_query_terms` | integer | 25 | Maximum number of terms in the generated query |
| `size` | integer | 10 | Number of similar documents to return |

Either `like` or `like_text` must be provided, but not both.

## Response

The response uses the standard search results format:

```json
{
  "results": [
    {
      "id": "article-17",
      "score": 8.45,
      "fields": {
        "title": "Building Async Web Services in Rust",
        "content": "Using tokio and hyper to build high-performance...",
        "author": "Charlie"
      }
    },
    {
      "id": "article-89",
      "score": 6.21,
      "fields": {
        "title": "Tokio Runtime Deep Dive",
        "content": "The tokio runtime provides async I/O primitives...",
        "author": "Dana"
      }
    }
  ],
  "total": 2
}
```

## How it works

1. **Term extraction**: Prism retrieves the source document (by ID or from provided text) and tokenizes the specified fields
2. **TF-IDF scoring**: Each term is scored using TF-IDF: `tf * (ln(N / (1 + df)) + 1)`, where `tf` is term frequency in the source, `N` is total documents, and `df` is document frequency
3. **Term filtering**: Terms below `min_term_freq` or `min_doc_freq` are excluded. Common stop words are naturally filtered by low IDF scores.
4. **Query construction**: The top `max_query_terms` terms (by TF-IDF score) are combined into a disjunction (OR) query
5. **Search execution**: The generated query is run against the collection
6. **Source exclusion**: When using `like` with a document ID, the source document is excluded from results

## Tuning parameters

### `min_term_freq`

How many times a term must appear in the source document to be considered significant. Increase this for long documents to focus on truly repeated terms.

| Value | Effect |
|-------|--------|
| 1 | Include all terms (noisy for long documents) |
| 2 | Default — require at least two occurrences |
| 3+ | Focus on frequently repeated terms |

### `min_doc_freq`

Minimum number of documents a term must appear in across the index. This filters out very rare terms that might be typos or noise.

| Value | Effect |
|-------|--------|
| 1 | Include rare terms |
| 5 | Default — exclude very rare terms |
| 10+ | Focus on well-established vocabulary |

### `max_query_terms`

The maximum number of terms to include in the generated query. More terms = broader matching but slower execution.

| Value | Effect |
|-------|--------|
| 10 | Focused — only the most significant terms |
| 25 | Default — good balance |
| 50+ | Broad matching, may include less significant terms |

## Use cases

### "Related articles" sidebar

```json
{
  "like": { "_id": "current-article-id" },
  "fields": ["title", "content"],
  "min_term_freq": 1,
  "max_query_terms": 15,
  "size": 5
}
```

### Content-based recommendations

```json
{
  "like_text": "User's reading history summary or interests description",
  "fields": ["content"],
  "max_query_terms": 30,
  "size": 20
}
```

### Duplicate detection

Use strict parameters to find near-duplicates:

```json
{
  "like": { "_id": "new-document" },
  "fields": ["content"],
  "min_term_freq": 3,
  "min_doc_freq": 2,
  "max_query_terms": 50,
  "size": 5
}
```

High-scoring results with these parameters are likely duplicates or near-duplicates.
