# Ingest Pipelines

Pipelines process documents before indexing, allowing transformations, enrichment, and validation.

## Pipeline Location

Pipelines are YAML files in `conf/pipelines/`:

```
<data_dir>/
└── conf/
    └── pipelines/
        ├── normalize-content.yaml
        └── extract-metadata.yaml
```

## Pipeline Definition

```yaml
name: normalize-content
description: "Normalize and clean content before indexing"

processors:
  - lowercase:
      field: title
  - html_strip:
      field: content
  - set:
      field: source
      value: "web-import"
  - remove:
      field: temp_field
  - rename:
      from: old_name
      to: new_name
```

## Available Processors

### lowercase

Convert field value to lowercase.

```yaml
- lowercase:
    field: title
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `field` | Yes | Field to transform |

**Before:** `{"title": "Hello WORLD"}`
**After:** `{"title": "hello world"}`

---

### html_strip

Remove HTML tags from field value.

```yaml
- html_strip:
    field: content
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `field` | Yes | Field to strip HTML from |

**Before:** `{"content": "<p>Hello <b>World</b></p>"}`
**After:** `{"content": "Hello World"}`

---

### set

Set a field to a fixed value.

```yaml
- set:
    field: source
    value: "web-import"
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `field` | Yes | Field to set |
| `value` | Yes | Value to set |

Overwrites existing value if field exists.

---

### remove

Remove a field from the document.

```yaml
- remove:
    field: internal_id
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `field` | Yes | Field to remove |

No error if field doesn't exist.

---

### rename

Rename a field.

```yaml
- rename:
    from: old_field_name
    to: new_field_name
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `from` | Yes | Original field name |
| `to` | Yes | New field name |

**Before:** `{"old_field_name": "value"}`
**After:** `{"new_field_name": "value"}`

---

## Using Pipelines

### At Index Time

Specify pipeline when indexing documents:

```bash
curl -X POST "http://localhost:3080/collections/articles/documents?pipeline=normalize-content" \
  -H "Content-Type: application/json" \
  -d '[{"title": "HELLO", "content": "<p>World</p>"}]'
```

### Default Pipeline

Set a default pipeline in the collection schema:

```yaml
collection: articles
indexing:
  default_pipeline: normalize-content
```

---

## Processor Order

Processors execute in order. Later processors see results of earlier ones:

```yaml
processors:
  # 1. First, strip HTML
  - html_strip:
      field: content
  # 2. Then lowercase (after HTML is removed)
  - lowercase:
      field: content
  # 3. Finally, add metadata
  - set:
      field: processed
      value: "true"
```

---

## Error Handling

If a processor fails:

- **Missing field** — Most processors skip silently
- **Type mismatch** — Document rejected with error
- **Invalid config** — Pipeline fails to load at startup

Check server logs for pipeline errors:

```bash
RUST_LOG=debug prism-server
```

---

## Example Pipelines

### Web Content Normalization

```yaml
name: web-normalize
description: "Clean web-scraped content"

processors:
  - html_strip:
      field: content
  - html_strip:
      field: title
  - lowercase:
      field: title
  - remove:
      field: raw_html
  - set:
      field: source_type
      value: "web"
```

### Log Processing

```yaml
name: log-processor
description: "Process log entries"

processors:
  - rename:
      from: "@timestamp"
      to: timestamp
  - rename:
      from: "log.level"
      to: level
  - lowercase:
      field: level
  - remove:
      field: agent
```

### Data Migration

```yaml
name: es-migration
description: "Transform Elasticsearch documents"

processors:
  - rename:
      from: _source.title
      to: title
  - rename:
      from: _source.body
      to: content
  - remove:
      field: _source
  - remove:
      field: _index
  - set:
      field: migrated_from
      value: "elasticsearch"
```

---

## See Also

- [Schema Reference](schema.md) — Collection configuration
- [prism-cli](../cli/prism-cli.md) — Document import
- [prism-import](../cli/prism-import.md) — Elasticsearch import
