# Suggestions & Autocomplete

The suggestions API provides prefix-based term completion and fuzzy "did you mean" corrections. Use it for search-as-you-type features.

## Endpoint

```
POST /collections/:collection/_suggest
```

## Basic prefix completion

```bash
curl -X POST http://localhost:3080/collections/articles/_suggest \
  -H "Content-Type: application/json" \
  -d '{
    "prefix": "mach",
    "field": "title",
    "size": 5
  }'
```

Response:

```json
{
  "suggestions": [
    { "term": "machine", "score": 1.0, "doc_freq": 142 },
    { "term": "machinery", "score": 1.0, "doc_freq": 23 },
    { "term": "machining", "score": 1.0, "doc_freq": 8 }
  ]
}
```

## Fuzzy suggestions ("did you mean")

Enable fuzzy matching to handle typos and misspellings:

```bash
curl -X POST http://localhost:3080/collections/articles/_suggest \
  -H "Content-Type: application/json" \
  -d '{
    "prefix": "machin lerning",
    "field": "content",
    "size": 5,
    "fuzzy": true,
    "max_distance": 2
  }'
```

Response:

```json
{
  "suggestions": [
    { "term": "machine", "score": 0.85, "doc_freq": 142 },
    { "term": "machining", "score": 0.72, "doc_freq": 8 }
  ],
  "did_you_mean": "machine learning"
}
```

## Request fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `prefix` | string | required | The prefix text to complete |
| `field` | string | required | Which indexed field to suggest from |
| `size` | integer | 5 | Maximum number of suggestions |
| `fuzzy` | boolean | false | Enable Levenshtein fuzzy matching |
| `max_distance` | integer | 2 | Maximum edit distance for fuzzy matching |

## Response fields

| Field | Type | Description |
|-------|------|-------------|
| `suggestions` | array | List of suggested terms |
| `suggestions[].term` | string | The suggested term |
| `suggestions[].score` | float | Relevance score (1.0 = exact prefix match, lower = fuzzy) |
| `suggestions[].doc_freq` | integer | Number of documents containing this term |
| `did_you_mean` | string? | Corrected query (only when `fuzzy: true`) |

## How it works

### Prefix matching

1. Prism scans the Tantivy term dictionary for the requested field
2. Uses a prefix-bounded range scan — only terms starting with the prefix are evaluated
3. Terms are scored by document frequency (more common terms rank higher)
4. Results are sorted by score and truncated to `size`

### Fuzzy matching

When `fuzzy: true` and the prefix yields few or no exact prefix matches:

1. Prism also runs Levenshtein distance corrections against the field vocabulary
2. Terms within `max_distance` edit operations are included
3. Fuzzy matches receive a lower score proportional to their edit distance
4. The `did_you_mean` field suggests a corrected version of the full input

## Use cases

### Search-as-you-type

Call the suggestions API on every keystroke (with debouncing):

```javascript
const response = await fetch('/collections/products/_suggest', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    prefix: inputValue,
    field: 'title',
    size: 8,
    fuzzy: inputValue.length > 3  // Enable fuzzy for longer inputs
  })
});
```

### Spell correction

Use `did_you_mean` to show a correction banner:

```
Showing results for "machin lerning"
Did you mean: machine learning?
```

### Field-specific suggestions

Request suggestions from different fields for different UI contexts:

```json
// Product name completion
{ "prefix": "iph", "field": "product_name", "size": 5 }

// Category completion
{ "prefix": "elec", "field": "category", "size": 5 }

// Author completion
{ "prefix": "ali", "field": "author", "size": 5 }
```

## Notes

- The field must be `indexed: true` in the collection schema
- Suggestions come from the Tantivy term dictionary (actual indexed terms, not stored values)
- For `text` fields, suggestions are individual tokens (words), not full field values
- For `string` fields, suggestions are the exact stored values
- Document frequency (`doc_freq`) helps rank popular terms higher
