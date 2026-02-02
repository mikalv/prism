# Ingest Pipelines: Document Preprocessing Before Indexing

> Issue #44

## Goal

Add a pipeline system that transforms and enriches documents before indexing.
Pipelines are defined as standalone YAML files, referenced via `?pipeline=name`
query parameter on index requests.

## Architecture

A new `pipeline` module in `prism/src/` loads YAML files from a configurable
directory (default: `conf/pipelines/`). Each pipeline is an ordered list of
processors. When a document is indexed with `?pipeline=name`, processors run
sequentially on the document *before* it reaches `CollectionManager.index()`.

```
API request (?pipeline=default)
  -> PipelineRegistry.get("default")
  -> for each processor: processor.process(&mut doc)
  -> CollectionManager.index(collection, processed_docs)
```

### Components

- **PipelineRegistry** — `HashMap<String, Pipeline>`, loaded at server startup.
- **Pipeline** — name, description, ordered `Vec<Box<dyn Processor>>`.
- **Processor trait** — `fn process(&self, doc: &mut Document) -> Result<()>`.

## Pipeline YAML Format

File: `conf/pipelines/normalize.yaml`

```yaml
name: normalize
description: Normalize text fields before indexing
processors:
  - lowercase:
      field: title
  - lowercase:
      field: content
  - html_strip:
      field: content
  - set:
      field: indexed_at
      value: "{{_now}}"
  - remove:
      field: _internal_notes
  - rename:
      from: old_field
      to: new_field
```

## Processors (First Iteration)

| Processor    | Config         | Behavior                                          |
|-------------|----------------|---------------------------------------------------|
| `lowercase` | `field`        | Convert string value to lowercase                 |
| `html_strip`| `field`        | Remove HTML tags, keep text content               |
| `set`       | `field, value` | Set field to value. `{{_now}}` = ISO8601 timestamp|
| `remove`    | `field`        | Remove field from document                        |
| `rename`    | `from, to`     | Rename field                                      |

Processors operate on `Document.fields` (HashMap<String, serde_json::Value>).
Non-string fields passed to `lowercase` or `html_strip` produce an error.

## API Changes

The existing index endpoint gains an optional query parameter:

```
POST /collections/{name}/documents?pipeline=normalize
```

Response changes from bare `201 Created` to JSON with details:

```json
{
  "indexed": 8,
  "failed": 2,
  "errors": [
    {"doc_id": "doc-3", "error": "html_strip: field 'content' is not a string"},
    {"doc_id": "doc-7", "error": "lowercase: field 'title' not found"}
  ]
}
```

When no `?pipeline` parameter is given, behavior is unchanged (no processing).

Unknown pipeline name returns `400 Bad Request`.

## Error Handling

- Each document is processed independently.
- If any processor fails on a document, that document is skipped.
- The error is collected and returned in the response.
- Remaining documents in the batch continue processing.

## Configuration

`PipelineRegistry` is initialized at server startup and injected into
`ApiServer`. The pipeline directory defaults to `conf/pipelines/` relative
to the config file location. No new config fields needed initially — the
directory is conventional.

## Out of Scope (Future Iterations)

- `embed` processor (hook into CachedEmbeddingProvider)
- `split` processor (document chunking for RAG)
- `script` processor (Lua/WASM custom logic)
- Schema-level default pipelines
- Pipeline versioning
- Hot-reload of pipeline definitions
