# Highlighting

Highlighting returns text snippets with matched query terms wrapped in HTML tags. This is useful for showing users why a result matched their query.

## Usage

Add a `highlight` object to your search request:

```bash
curl -X POST http://localhost:3080/collections/articles/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "machine learning",
    "limit": 10,
    "highlight": {
      "fields": ["title", "content"],
      "pre_tag": "<mark>",
      "post_tag": "</mark>",
      "fragment_size": 150,
      "number_of_fragments": 3
    }
  }'
```

## Configuration options

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `fields` | string[] | required | Which fields to generate highlights for |
| `pre_tag` | string | `"<b>"` | Opening tag for matched terms |
| `post_tag` | string | `"</b>"` | Closing tag for matched terms |
| `fragment_size` | integer | 150 | Maximum characters per snippet fragment |
| `number_of_fragments` | integer | 3 | Maximum number of fragments per field |

## Response

Highlighted fields appear in the `highlight` field of each result. Each field maps to an array of snippet fragments:

```json
{
  "results": [
    {
      "id": "doc-1",
      "score": 4.82,
      "fields": {
        "title": "Introduction to Machine Learning",
        "content": "Machine learning is a branch of artificial intelligence..."
      },
      "highlight": {
        "title": [
          "Introduction to <mark>Machine</mark> <mark>Learning</mark>"
        ],
        "content": [
          "<mark>Machine</mark> <mark>learning</mark> is a branch of artificial intelligence that focuses on...",
          "Supervised <mark>learning</mark> uses labeled training data to...",
          "Deep <mark>learning</mark> is a subset of <mark>machine</mark> <mark>learning</mark> that uses neural networks..."
        ]
      }
    }
  ],
  "total": 42
}
```

## How it works

Prism uses Tantivy's built-in `SnippetGenerator` for highlighting. For each requested field:

1. The query is parsed and applied to the field
2. Tantivy identifies the positions of matching terms in the stored text
3. Relevant fragments are extracted around the matched terms
4. Matched terms are wrapped in the configured tags

## Notes

- Only `text` fields with `stored: true` and `indexed: true` can be highlighted
- If a requested field doesn't exist, has no matches, or isn't stored, it's omitted from the `highlight` map
- The `highlight` field is omitted entirely from results that have no highlights
- Highlighting adds some overhead; request it only for fields you need
- Fragment boundaries respect word boundaries when possible

## Custom tags

Use any tags you need for your frontend:

```json
{
  "highlight": {
    "fields": ["content"],
    "pre_tag": "<span class=\"match\">",
    "post_tag": "</span>"
  }
}
```

Or for terminal/plain-text use:

```json
{
  "highlight": {
    "fields": ["content"],
    "pre_tag": "**",
    "post_tag": "**"
  }
}
```
