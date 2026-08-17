# Plan: ES-compat index create/delete endpoints

> **Note (2026-08-16):** Since the Kibana bring-up, ES-compat endpoints are served at their standard Elasticsearch paths on the root router — the `/_elastic` prefix referenced below was removed.

**Status:** Draft for review
**Goal:** Add `PUT /{index}` and `DELETE /{index}` to the `/_elastic`
compatibility layer so that apps speaking the ES REST API can manage the
Prism collections they search, without dropping into Prism's native API.

The user wants ES-mapping → Prism-schema translation (not a "default text
backend" shortcut), so that mappings a client actually sends are honored.

---

## Context (verified against source)

### What Prism already has

- **Native create:** `POST /collections/{name}` in `prism/src/api/routes.rs:862`
  (`create_collection`). Takes a full `CollectionSchema`, requires a non-empty
  `backends`, persists schema + data. Returns 201.
- **Native delete:** `DELETE /collections/{name}?delete_data=true|false}` at
  `routes.rs:929` (`delete_collection`). Calls `manager.remove_collection()`
  (in-memory), `delete_collection_data()` (on-disk), and
  `remove_schema_file()` (best-effort). Returns 200.
- **ES-compat layer:** `prism-es-compat/` — all endpoints are thin handlers
  returning `Result<_, EsCompatError>`. State is `EsCompatState { manager }`.
  Routes registered in `prism-es-compat/src/router.rs`. The product header
  `X-Elastic-Product: Elasticsearch` is stamped on every response via a
  response middleware, so new routes inherit it for free.

### The mapping mismatch

ES `PUT /{index}` and Prism `CollectionSchema` disagree in three ways. The
plan must resolve each explicitly:

| Dimension | ES `PUT /{index}` | Prism `CollectionSchema` |
|---|---|---|
| Body | Optional; empty body = default mapping | `backends` required |
| Fields | Auto-detected from first doc if absent | Declared up-front in schema |
| Structure | One flat `mappings.properties` map | Split into `backends.text.fields` / `backends.vector` |

**Resolution:** translate ES `mappings.properties` → `TextBackendConfig.fields`.
Empty body → a sane default (see "Default schema" below). We do not auto-create
a vector backend from an ES mapping — ES mappings have no vector concept, so
this would be invented behavior. Vector collections stay a native-API concern.

### Prism field model (`prism/src/schema/types.rs`)

```
TextBackendConfig { fields: Vec<TextField>, bm25_k1, bm25_b }
TextField { name, field_type: FieldType, stored, indexed, tokenizer, tokenizer_options }
FieldType = Text | String | I64 | U64 | F64 | Bool | Date | Bytes
```

Note there is **no separate keyword type** in Prism. ES `keyword` maps to
`String` + `indexed: true` + `tokenizer: Raw` (exact match). This is the
single most important translation rule.

### ES mapping types we need to handle

ES 7.x field types a generic app is likely to send:

| ES type | Prism `FieldType` | Notes |
|---|---|---|
| `text` | `Text` | indexed + tokenized (`TokenizerType::Default`) |
| `keyword` | `String` | indexed, `tokenizer: Raw` (exact match) |
| `integer`, `long` | `I64` | |
| `unsigned_long` | `U64` | |
| `float`, `double`, `scaled_float` | `F64` | `scaled_float` ignores `scaling_factor` |
| `boolean` | `Bool` | |
| `date` | `Date` | accept `format`; stored for range queries |
| `binary` | `Bytes` | |
| `constant_keyword` | `String` | treat as keyword |
| `wildcard` | `String` | treat as keyword (raw) |
| `ip` | `String` | store as raw string; no native IP type |
| `object` / `nested` | *(skip with warning)* | Prism stores fields flat — no nested. Log and skip the sub-tree. |
| `flattened` | *(skip with warning)* | no flat-of-objects type |
| `alias` | *(skip with warning)* | no field aliasing |
| `search_as_you_type` | `Text` | fall back to plain text |
| `dense_vector` | *(error)* | no ES-native vector → Prism vector; instruct user to use native API |

### ES response shape (what clients expect)

```json
// PUT /{index} → 200 OK
{ "acknowledged": true, "shards_acknowledged": true, "index": "myindex" }

// DELETE /{index} → 200 OK
{ "acknowledged": true }
```

This matches ES 7.x exactly. (`shards_acknowledged` is always `true` since
Prism has no shard-acknowledgment phase.)

---

## Design decisions

### D1: Empty body = default text-backend schema

When a client does `PUT /index` with no body (common — ES accepts this), we
create a collection with a single-field text backend:

```yaml
backends:
  text:
    fields:
      - { name: id, type: string, indexed: true, stored: true }
      - { name: content, type: text, indexed: true, stored: true }
```

This is the minimum viable schema: the subsequent `POST /{index}/_doc` with an
arbitrary JSON object will index whatever fields the doc has, with `id` +
`content` guaranteed present. Rationale: ES's empty-body default is a dynamic
mapping; Prism can't do dynamic mapping cheaply, so we pick the most useful
fixed default. Documents with extra fields are still indexed (Prism's text
backend accepts undeclared fields into a catch-all by default — to be verified
in implementation).

### D2: `keyword` → `String` + Raw tokenizer

The most-used ES type after `text`. Correct translation:

```rust
FieldType::String, stored: false, indexed: true, tokenizer: Some(TokenizerType::Raw)
```

`stored: false` matches ES keyword default (keywords are indexed for
aggregation/filtering but not stored separately — they come back via
`_source`).

### D3: Untranslatable types — skip with warning, don't fail

For `object`/`nested`/`flattened`/`alias`: log a `tracing::warn!` with the
field name and type, and **omit** the field from the schema. The collection is
still created. Rationale: failing the whole `PUT` because one field is nested
would break many real ES setups. The caller can still index those docs (the
nested values are flattened at index time by the existing ES-compat doc
handlers — to be confirmed).

For `dense_vector`: return **400 with a clear message** pointing to the native
`/collections/{c}/search` vector API. We do not silently invent a vector
backend from an ES mapping.

### D4: Idempotency / conflict semantics

`PUT /{index}` when the index already exists → **400** with ES-style error
body:

```json
{
  "error": {
    "type": "resource_already_exists_exception",
    "reason": "index [myindex/...] already exists",
    "root_cause": [{ "type": "resource_already_exists_exception", "reason": "..." }]
  },
  "status": 400
}
```

This requires a new `EsCompatError::IndexAlreadyExists` variant. `head_index_handler`
already checks existence via `list_collections().contains(&index)` — reuse that
check.

### D5: DELETE is a thin wrapper

`DELETE /{index}` calls native `delete_collection` semantics with
`delete_data: true` (ES semantics = remove data). We do **not** expose the
`?delete_data=false` query param — ES clients don't know about it; if a user
wants to keep data they use the native API. Map `CollectionNotFound` → ES
`index_not_found_exception` (404).

### D6: No settings / aliases / templates

This plan covers only `PUT /{index}` (with `mappings`). It does **not**
implement:
- `PUT /{index}/_settings` (no-op return 200? or 400?) — **decide later**
- `PUT /_index_template`, `PUT /_alias`, `PUT /{index}/_mapping` (post-create)
- Dynamic-mapping settings (`"dynamic": "strict"` etc.)

These are explicitly out of scope for this slice. The feature matrix already
marks `PUT /{index}/_mapping` as ❌; we keep that.

---

## Implementation plan

### Files to change

| File | Change |
|---|---|
| `prism-es-compat/src/endpoints/cluster.rs` *(or new `index_management.rs`)* | New `create_index_handler` + `delete_index_handler` |
| `prism-es-compat/src/query/types.rs` *(or new `mapping.rs`)* | New `EsCreateIndexRequest`, `EsMapping`, `EsFieldSpec` deserialize types |
| `prism-es-compat/src/endpoints/mod.rs` | Re-export new handlers |
| `prism-es-compat/src/router.rs` | Add `PUT /:index` and `DELETE /:index` routes |
| `prism-es-compat/src/error.rs` | Add `IndexAlreadyExists(String)` variant + map to 400 |
| `prism-es-compat/src/lib.rs` | Re-export if needed |
| `prism/docs/guides/elasticsearch-compat.md` | Flip matrix rows from ❌ to ✅; add notes |

### Step-by-step

**Step 1 — Types.** Add ES mapping request types. Mirror ES's shape exactly so
standard clients deserialize without surprises:

```rust
// In a new prism-es-compat/src/mapping.rs or in query/types.rs
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EsCreateIndexRequest {
    #[serde(default)]
    pub mappings: Option<EsMappings>,
    #[serde(default)]
    pub settings: Option<serde_json::Value>, // accepted, ignored
    #[serde(default)]
    pub aliases: Option<serde_json::Value>,  // accepted, ignored
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EsMappings {
    #[serde(default)]
    pub properties: HashMap<String, EsFieldSpec>,
    #[serde(default)]
    pub dynamic: Option<String>, // accepted, ignored (always "true" in effect)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EsFieldSpec {
    /// Shorthand: { "field": "text" }
    TypeOnly(String),
    /// Full form: { "field": { "type": "text", ... } }
    Object {
        #[serde(rename = "type")]
        field_type: String,
        #[serde(default)]
        format: Option<String>,        // dates
        #[serde(default)]
        scaling_factor: Option<f64>,   // scaled_float (ignored)
        #[serde(default)]
        index: Option<bool>,
        #[serde(default)]
        store: Option<bool>,
        #[serde(default)]
        analyzer: Option<String>,      // accepted, ignored (we use our tokenizer)
        #[serde(default)]
        properties: Option<HashMap<String, EsFieldSpec>>, // nested — for skip-with-warning
    },
}

impl EsFieldSpec {
    pub fn type_name(&self) -> &str {
        match self { Self::TypeOnly(t) => t, Self::Object { field_type, .. } => field_type }
    }
}
```

**Step 2 — Translation.** Pure function, no I/O — easy to unit test:

```rust
fn translate_mapping(req: &EsCreateIndexRequest, index: &str)
    -> Result<CollectionSchema, EsCompatError>
{
    let mut fields = match &req.mappings {
        None => default_fields(),
        Some(m) if m.properties.is_empty() => default_fields(),
        Some(m) => {
            let mut out = Vec::with_capacity(m.properties.len());
            for (name, spec) in &m.properties {
                match translate_field(name, spec) {
                    Ok(f) => out.push(f),
                    Err(Skip(msg)) => tracing::warn!(field = %name, "{msg}"),
                    Err(Fatal(e)) => return Err(e), // dense_vector etc.
                }
            }
            out
        }
    };
    Ok(CollectionSchema {
        collection: index.to_string(),
        backends: Backends { text: Some(TextBackendConfig { fields, ..Default::default() }), vector: None, graph: None },
        ..Default::default()
    })
}

fn translate_field(name: &str, spec: &EsFieldSpec) -> Result<TextField, FieldError> {
    use FieldType::*;
    match spec.type_name() {
        "text"              => Ok(text_field(name, Default, true, true)),  // indexed, stored
        "keyword"           => Ok(raw_string_field(name, true, false)),
        "constant_keyword"  => Ok(raw_string_field(name, true, false)),
        "wildcard"          => Ok(raw_string_field(name, true, false)),
        "ip"                => Ok(raw_string_field(name, true, false)),
        "integer" | "long"  => Ok(TextField { name: name.into(), field_type: I64, indexed: true, stored: true, ..Default::default() }),
        "unsigned_long"     => Ok(TextField { name: name.into(), field_type: U64, indexed: true, stored: true, ..Default::default() }),
        "float" | "double" | "scaled_float" => Ok(TextField { name: name.into(), field_type: F64, indexed: true, stored: true, ..Default::default() }),
        "boolean"           => Ok(TextField { name: name.into(), field_type: Bool, indexed: true, stored: true, ..Default::default() }),
        "date"              => Ok(TextField { name: name.into(), field_type: Date, indexed: true, stored: true, ..Default::default() }),
        "binary"            => Ok(TextField { name: name.into(), field_type: Bytes, indexed: false, stored: true, ..Default::default() }),
        "search_as_you_type"=> Ok(text_field(name, Default, true, true)),
        "object" | "nested" | "flattened" | "alias" => Err(FieldError::Skip(format!("{:?} not supported (Prism stores fields flat)", spec.type_name()))),
        "dense_vector"      => Err(FieldError::Fatal(EsCompatError::UnsupportedFieldType(name.into(), "dense_vector — use native /collections API for vector collections".into()))),
        other               => Err(FieldError::Skip(format!("unknown ES type '{other}' — skipped"))),
    }
}
```

**Step 3 — Handlers.** Following the `head_index_handler` / native
`create_collection` patterns:

```rust
pub async fn create_index_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
    body: Option<Json<serde_json::Value>>,  // tolerate empty body
) -> Result<Json<EsCreateIndexResponse>, EsCompatError> {
    // 1. Existence check (reuse head_index_handler's pattern)
    if state.manager.list_collections().contains(&index) {
        return Err(EsCompatError::IndexAlreadyExists(index));
    }
    // 2. Parse body (empty -> default)
    let req: EsCreateIndexRequest = match body {
        None | Some(Json(serde_json::Value::Null)) => EsCreateIndexRequest::default(),
        Some(Json(v)) => serde_json::from_value(v)
            .map_err(|e| EsCompatError::InvalidRequestBody(e.to_string()))?,
    };
    // 3. Translate
    let schema = translate_mapping(&req, &index)?;
    // 4. Create via manager (same path as native handler)
    state.manager.add_collection(schema.clone()).await
        .map_err(|e| EsCompatError::Internal(e.to_string()))?;
    if let Err(e) = state.manager.persist_schema(&schema) {
        tracing::warn!(index = %index, "schema not persisted: {e}");
    }
    Ok(Json(EsCreateIndexResponse { acknowledged: true, shards_acknowledged: true, index }))
}

pub async fn delete_index_handler(
    State(state): State<EsCompatState>,
    Path(index): Path<String>,
) -> Result<Json<EsDeleteIndexResponse>, EsCompatError> {
    state.manager.remove_collection(&index).await?; // Error::CollectionNotFound → 404
    if let Err(e) = state.manager.delete_collection_data(&index) {
        tracing::error!(index = %index, "data delete failed: {e}");
        return Err(EsCompatError::Internal(e.to_string()));
    }
    if let Err(e) = state.manager.remove_schema_file(&index) {
        tracing::warn!(index = %index, "schema file not removed: {e}");
    }
    Ok(Json(EsDeleteIndexResponse { acknowledged: true }))
}
```

**Step 4 — Routes.** In `router.rs`, add to the `Router::new()` builder (note:
axum needs `.put(...)` chained on the existing `/:index` route, or a separate
route):

```rust
.route("/:index", head(head_index_handler).put(create_index_handler).delete(delete_index_handler))
```

⚠️ **Conflict to resolve during impl:** `/:index` currently has only `head`.
Adding `put` + `delete` is fine, but verify no handler-signature clashes with
axum's method routing. If problematic, register `PUT /:index` and
`DELETE /:index` as separate `.route(...)` calls (axum allows the same path in
multiple `.route()` calls as long as methods don't overlap).

**Step 5 — Error variant.** Add to `EsCompatError`:

```rust
#[error("Index already exists: {0}")]
IndexAlreadyExists(String),
#[error("Unsupported field type for {0}: {1}")]
UnsupportedFieldType(String, String),
```

Map in `IntoResponse` impl:
- `IndexAlreadyExists` → 400 `resource_already_exists_exception`
- `UnsupportedFieldType` → 400 `mapper_parsing_exception`
- Existing `IndexNotFound` (from native `Error::CollectionNotFound`) → 404
  `index_not_found_exception` — verify this mapping already exists or add it.

**Step 6 — Tests.** Unit + integration:

Unit (`mapping.rs`):
- `translate_field` for every ES type in the table above (success + skip + fatal)
- empty body → default schema
- `keyword` → `String` + `Raw` tokenizer
- `object` field → skipped, others still present
- `dense_vector` → `Err(Fatal)`

Integration (new `prism-es-compat/tests/index_management.rs` or extend
`prism/tests/`):
- `PUT /_elastic/idx` empty body → 200, then `GET /_elastic/_cat/indices`
  lists `idx`
- `PUT /_elastic/idx` with `{"mappings": {"properties": {"title": {"type":
  "text"}, "tags": {"type": "keyword"}, "ts": {"type": "date"}}}}` → 200,
  index a doc, search by `tags` exact + `title` match
- `PUT /_elastic/idx` twice → second is 400
  `resource_already_exists_exception`
- `DELETE /_elastic/idx` → 200, then `HEAD /_elastic/idx` → 404
- `DELETE /_elastic/missing` → 404 `index_not_found_exception`
- `PUT` with `{"mappings": {"properties": {"v": {"type": "dense_vector",
  "dims": 4}}}}` → 400 with helpful message
- Verify `X-Elastic-Product: Elasticsearch` header on all responses (the
  middleware covers this, but assert it in one test)

**Step 7 — Docs.** In `docs/guides/elasticsearch-compat.md`:
- REST endpoints matrix: flip `PUT /{index}` and `DELETE /{index}` from ❌ to ✅
- Add a "Creating indices from ES mappings" subsection under "Response Format"
  with the ES-type → Prism-type table and the default-schema behavior
- Add the `object`/`nested`/`dense_vector` notes
- Update "Limitations" to remove "Index creation must be done via Prism's
  native API"

### Open questions for review

1. **Default schema fields** (D1): is `id` + `content` the right default, or
   should empty-body creation be rejected (forcing the client to send a
   mapping)? ES allows empty body; rejecting would be less compatible.
2. **Undeclared fields at index time**: does Prism's text backend silently
   index fields not in the schema (catch-all), or drop them? This affects
   whether the default schema in D1 actually works for arbitrary docs. **Must
   verify before Step 3.**
3. **`PUT /{index}/_settings`**: do we return 200 (no-op) or 400? Out of
   scope per D6 but a client may hit it. Recommend 200 no-op for compat.
4. **`dynamic: "strict"`**: ES would reject docs with unmapped fields. Prism
   can't enforce this. Silently ignore for now?

### Non-goals

- Vector backend creation from ES mappings (`dense_vector` errors clearly)
- Settings, aliases, templates, dynamic-mapping policies
- `PUT /{index}/_mapping` (post-create schema changes) — Prism schemas are
  immutable post-create
- Partial updates / reindex API

### Estimated size

~250 lines of new code (types + translator + 2 handlers + error variants) +
~200 lines of tests. One file of source + test additions. ~30 min docs update.
