---
name: prism-code-search
description: Use when indexing and searching source code in Prism. Covers tree-sitter code tokenizers (code-treesitter, code), schema setup for code collections, camelCase/snake_case sub-token search, language auto-detection, comment/string indexing options, and re-embedding existing collections.
---

# Prism Code Search (Tree-sitter)

How to index and search source code with AST-aware tokenization in Prism.

## Why a code tokenizer?

Default text tokenizers split on whitespace/punctuation only, so `getUserById`
is one opaque token. Prism's code tokenizers split identifiers on
camelCase / snake_case / PascalCase boundaries: `getUserById` →
`get`, `user`, `by`, `id`. This makes sub-token search work —
query `user` matches documents containing `getUserById`.

Two tokenizers are available:

| Tokenizer | Approach | Feature |
|-----------|----------|---------|
| `code-treesitter` | AST-aware (tree-sitter) | `tokenizer-treesitter` |
| `code` | Regex/heuristic splitting, no grammar | built-in |

`code-treesitter` distinguishes identifiers from comments, strings, and
keywords; `code` treats the whole document as text and splits fragments.

**Feature default:** `tokenizer-treesitter` is enabled by default in
`prism-server` builds since v0.7.0. For library/CLI builds, opt in:

```bash
cargo build --release --features tokenizer-treesitter
```

Without the feature, Prism falls back to the `code` tokenizer with a warning,
and `code-treesitter` schemas silently degrade — check server logs for
`falling back` warnings on startup or first index.

## Creating a code collection

### Minimal schema (auto-detect language)

```yaml
collection: my-code
backends:
  text:
    fields:
      - name: file_path
        type: string
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true   # required — default is false!
        tokenizer: code-treesitter
```

```bash
curl -X PUT http://localhost:3080/collections/my-code -H "Content-Type: application/json" -d @- << 'EOF'
{
  "collection": "my-code",
  "backends": {
    "text": {
      "fields": [
        {"name": "file_path", "type": "string", "stored": true, "indexed": true},
        {"name": "content", "type": "text", "stored": true, "indexed": true, "tokenizer": "code-treesitter"}
      ]
    }
  }
}
EOF
```

### Explicit language + comment/string control

```yaml
backends:
  text:
    fields:
      - name: content
        type: text
        stored: true
        indexed: true
        tokenizer: code-treesitter
        tokenizer_options:
          language: rust            # or python, go, typescript, ...
          index_comments: true      # default true
          index_strings: true       # default true
```

| Option | Type | Default | Notes |
|--------|------|---------|-------|
| `language` | string | auto-detect | Shebang + keyword heuristics; explicit is more accurate |
| `index_comments` | bool | `true` | Index comment text |
| `index_strings` | bool | `true` | Index string literal content |

## What gets indexed

1. **Identifiers** — function/variable/type names. Split on
   camelCase/snake_case/PascalCase boundaries, lowercased:
   `ConnectionPool` → `connection`, `pool`; `max_connections` → `max`, `connections`.
2. **Comments** (if `index_comments`) — natural-language words, plus
   camelCase fragments inside comments are split too:
   `// validates validateZebraInput` → `validates`, `validate`, `zebra`, `input`.
3. **String literals** (if `index_strings`) — same treatment as comments:
   `"kangarooJumpHigh"` → `kangaroo`, `jump`, `high`.
4. **Keywords** — emitted as-is.

**Search semantics (v0.7.0+):** only sub-tokens are indexed for
camelCase fragments — the concatenated lowercase form is NOT indexed.
Query `validatezebrainput` will NOT match; query `zebra` will.
Plain lowercase words are indexed as whole tokens (`carefully` matches
`carefully`).

## Supported languages (16)

rust, python, javascript, typescript, go, c, cpp, ruby, elixir, erlang,
bash, sql, yaml, toml, json, html

Auto-detection covers `.rs/.py/.js/.ts/...` via extension plus shebang and
keyword heuristics when content is indexed without a `language` option.

## Indexing and searching

```bash
# Index a file (id = path, content = source)
curl -X POST "http://localhost:3080/collections/my-code/documents?sync=true" \
  -H "Content-Type: application/json" \
  -d '{"documents":[{"id":"src/main.rs","fields":{"file_path":"src/main.rs","content":"fn process_order() { /* validates validateZebraInput */ }"}}]}'

# Sub-token search — hits
curl -X POST http://localhost:3080/collections/my-code/search \
  -H "Content-Type: application/json" -d '{"query":"zebra"}'   # → matches

# Full concatenated form — does NOT hit (v0.7.0+ semantics)
curl -X POST http://localhost:3080/collections/my-code/search \
  -H "Content-Type: application/json" -d '{"query":"validatezebrainput"}'  # → 0 hits
```

## Re-indexing existing collections

The tokenizer applies at index time. Documents indexed with an older
tokenizer (or older splitting semantics) keep their tokens until re-indexed.
Use the reindex admin endpoint or re-embed via `POST /admin/vectorize`
(vector part) — for pure text-tokenizer changes, rebuild the collection:

1. Export: `prism-cli collection export my-code -o my-code.prism.jsonl`
2. Delete + recreate collection with the desired schema
3. Import: `prism-cli collection restore -f my-code.prism.jsonl`

Existing ws_*_code_* collections on the production instance
(192.168.88.212:3080) were indexed before v0.7.0 camelCase-split semantics;
they must be re-imported to pick up comment/string sub-token splitting.

## Feature flags per language

Each grammar is its own feature in the `prism-treesitter` crate
(`rust`, `python`, ..., `sql`). All are on by default when the crate is
included. `prism-server` enables `tokenizer-treesitter` (all grammars) by
default since v0.7.0.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Search returns 0 hits for everything | `indexed: true` missing on the field | Set `indexed: true` on text fields |
| `code-treesitter` silently behaves like `code` | Feature not compiled in | Build with `--features tokenizer-treesitter` (default for prism-server ≥ v0.7.0) |
| Concatenated queries (`getuserbyid`) miss | Expected v0.7.0+ semantics | Query sub-tokens (`user`), or wildcard if supported |
| Wrong language detected | Auto-detect heuristics failed | Set `tokenizer_options.language` explicitly |
| Old collections miss new sub-tokens | Indexed before semantic change | Re-export/re-import (see above) |
