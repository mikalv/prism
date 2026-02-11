# Code Search

Prism includes specialized tokenizers for indexing and searching source code. Unlike default text tokenizers that split on whitespace and punctuation, code tokenizers understand programming conventions like `camelCase`, `snake_case`, and `PascalCase`, splitting identifiers into searchable parts.

## Tokenizers

Prism offers two code tokenizers:

| Tokenizer | Description | Feature required |
|-----------|-------------|------------------|
| `code` | Regex-based identifier splitting | Built-in (always available) |
| `code-treesitter` | AST-aware parsing with tree-sitter | `tokenizer-treesitter` |

### The `code` tokenizer

The built-in `code` tokenizer splits identifiers on camelCase, snake_case, and PascalCase boundaries, then lowercases each part. For example, `getUserName` becomes `[get, user, name]`.

```yaml
backends:
  text:
    fields:
      - name: source_code
        type: text
        stored: true
        indexed: true
        tokenizer: code
```

### The `code-treesitter` tokenizer

The tree-sitter tokenizer parses code into an AST (Abstract Syntax Tree) before extracting tokens. This gives it real language awareness — it can distinguish identifiers from comments, string literals, and keywords.

!!! note "Feature flag required"
    The tree-sitter tokenizer requires building Prism with the `tokenizer-treesitter` feature:
    ```bash
    cargo build --release --features tokenizer-treesitter
    ```
    Without this feature, Prism will fall back to the `code` tokenizer with a warning.

## Supported languages

The tree-sitter tokenizer supports 16 languages:

| Language | Schema name | File extensions |
|----------|-------------|-----------------|
| Rust | `rust` | `.rs` |
| Python | `python` | `.py` |
| JavaScript | `javascript` | `.js`, `.jsx` |
| TypeScript | `typescript` | `.ts`, `.tsx` |
| Go | `go` | `.go` |
| C | `c` | `.c`, `.h` |
| C++ | `cpp` | `.cpp`, `.cc`, `.cxx`, `.hpp` |
| Ruby | `ruby` | `.rb` |
| Elixir | `elixir` | `.ex`, `.exs` |
| Erlang | `erlang` | `.erl`, `.hrl` |
| Bash | `bash` | `.sh`, `.bash` |
| SQL | `sql` | `.sql` |
| YAML | `yaml` | `.yaml`, `.yml` |
| TOML | `toml` | `.toml` |
| JSON | `json` | `.json` |
| HTML | `html` | `.html`, `.htm` |

Each language is compiled behind its own feature flag in the `prism-treesitter` crate. All are enabled by default when the crate is included.

## Schema configuration

### Basic usage (auto-detect language)

```yaml
collection: codebase
description: "Source code repository"

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
        indexed: true
        tokenizer: code-treesitter
```

When no language is specified, the tokenizer auto-detects the language from the content using shebang lines and keyword heuristics.

### Explicit language

If you know the language ahead of time (recommended for best accuracy):

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
          language: rust
```

### Controlling what gets indexed

By default, the tree-sitter tokenizer indexes identifiers, comments, and string literals. You can disable comments or strings:

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
          language: python
          index_comments: false
          index_strings: false
```

### Tokenizer options reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `language` | string | auto-detect | Language name (e.g. `rust`, `python`, `go`). When omitted, detected from content. |
| `index_comments` | bool | `true` | Include comment text in the index |
| `index_strings` | bool | `true` | Include string literal content in the index |

## How it works

The tree-sitter tokenizer processes code in three steps:

1. **Parse** — Tree-sitter parses the source code into an AST
2. **Extract** — The tokenizer walks the AST and collects:
   - **Identifiers** (function names, variables, types) — split on camelCase/snake_case boundaries and lowercased
   - **Comments** (if enabled) — tokenized as natural language text
   - **String literals** (if enabled) — tokenized as natural language text
   - **Keywords** — emitted as-is
3. **Emit** — Each sub-token is emitted with its byte offset from the AST node

### Example

Given this Rust code:

```rust
fn getUserName(user_id: u64) -> String {
    // Fetch the user name from database
    format!("user_{}", user_id)
}
```

The tokenizer produces these tokens:

| Source | Tokens |
|--------|--------|
| `getUserName` | `get`, `user`, `name` |
| `user_id` | `user`, `id` |
| `u64` | `u64` |
| `String` | `string` |
| `// Fetch the user name from database` | `fetch`, `the`, `user`, `name`, `from`, `database` |
| `"user_{}"` | `user` |

Searching for `user name` would match this function because both `user` and `name` appear as tokens from the identifier `getUserName`.

### Fallback behavior

If the tree-sitter parser cannot determine the language or fails to parse the content, the tokenizer falls back to simple whitespace + identifier splitting. This ensures that indexing never fails — all code gets indexed, just with less precision for unrecognized languages.

## Examples

### Multi-language repository

Index different file types with explicit languages using separate fields:

```yaml
collection: monorepo
description: "Multi-language monorepo"

backends:
  text:
    fields:
      - name: path
        type: string
        stored: true
        indexed: true
      - name: language
        type: string
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true
        tokenizer: code-treesitter
```

Using auto-detection with a single `content` field works well for mixed-language repositories.

### Code-only indexing (no comments/strings)

For pure identifier search, disable comments and strings:

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
          index_comments: false
          index_strings: false
```

### Searching indexed code

Once indexed, search works the same as any other Prism collection:

```bash
curl -X POST http://localhost:3080/collections/codebase/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "parse config",
    "limit": 20
  }'
```

This would find functions like `parseConfig`, `parse_config`, `ParseConfiguration`, and comments mentioning "parse" and "config".

## See also

- [Collection Schema](../reference/schema.md) — Full schema reference
- [Search](search.md) — Search API documentation
- [Hybrid Search](hybrid-search.md) — Combining text and vector search
