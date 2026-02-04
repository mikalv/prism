# Code-Aware Tokenizer Design

> **Related issues:** #66 (code-simple), #70 (tree-sitter future)

## Overview

Add configurable code-aware tokenizers that understand programming language syntax for better code search results. Standard text tokenizers split on whitespace and punctuation, which doesn't work well for code identifiers like `camelCase` and `snake_case`.

## Use Cases

1. **Source code search** - GitHub-style code search
2. **Documentation with code examples** - docs with inline code
3. **Log search** - stack traces, error messages with code references

## Architecture

```
prism/src/
├── tokenizer/
│   ├── mod.rs           # TokenizerRegistry, Tokenizer trait
│   ├── code_simple.rs   # Regex-based implementation
│   └── default.rs       # Wrapper around Tantivy's default
```

### TokenizerRegistry

Registers all available tokenizers at startup. Tantivy's `Index` receives these via `register_tokenizer()`.

### Tokenizer Selection

Configurable per field in schema:

```yaml
backends:
  text:
    fields:
      - name: code
        type: text
        tokenizer: code-simple
      - name: description
        type: text
        # uses "default" when not specified
```

## Code-Simple Tokenizer

### Token Generation Rules

| Input | Output tokens |
|-------|---------------|
| `getUserName` | `getUserName`, `get`, `user`, `name` |
| `get_user_name` | `get_user_name`, `get`, `user`, `name` |
| `get-user-name` | `get-user-name`, `get`, `user`, `name` |
| `HTTP2Connection` | `HTTP2Connection`, `http`, `2`, `connection` |
| `parseJSON` | `parseJSON`, `parse`, `json` |

Key principles:
- **Preserve original** + emit split tokens (exact match works)
- **Lowercase** split tokens for case-insensitive search
- **Numbers** as separate tokens (useful for versions, error codes)

### Implementation

```rust
pub struct CodeSimpleTokenizer;

impl Tokenizer for CodeSimpleTokenizer {
    type TokenStream<'a> = CodeSimpleTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        CodeSimpleTokenStream::new(text)
    }
}
```

### Split Logic

1. Split on whitespace and standard separators
2. For each token:
   - Emit original token (unchanged)
   - Split camelCase/PascalCase → emit lowercase parts
   - Split snake_case/kebab-case → emit lowercase parts
   - Split numbers from letters (`HTTP2` → `http`, `2`)

### Regex Patterns

```rust
// camelCase split: "getUser" -> ["get", "User"]
r"([a-z])([A-Z])"

// Acronym split: "HTTPServer" -> ["HTTP", "Server"]
r"([A-Z]+)([A-Z][a-z])"

// snake/kebab split
r"[_\-]"
```

## Schema Changes

```rust
// prism/src/schema/mod.rs
pub struct FieldDefinition {
    pub name: String,
    pub field_type: FieldType,
    pub indexed: bool,
    pub stored: bool,
    pub tokenizer: Option<String>,  // "default", "code-simple", etc.
}
```

`TextBackend` reads the `tokenizer` field and uses the correct tokenizer during indexing.

## Testing

### Unit Tests

```rust
#[test]
fn test_camel_case() {
    assert_tokens("getUserName", &["getUserName", "get", "user", "name"]);
}

#[test]
fn test_snake_case() {
    assert_tokens("get_user_name", &["get_user_name", "get", "user", "name"]);
}

#[test]
fn test_acronyms() {
    assert_tokens("HTTPServer", &["HTTPServer", "http", "server"]);
    assert_tokens("parseJSON", &["parseJSON", "parse", "json"]);
}

#[test]
fn test_mixed() {
    assert_tokens("HTTP2Connection", &["HTTP2Connection", "http", "2", "connection"]);
}
```

### Integration Test

Create collection with `tokenizer: code-simple`, index code, search for parts of identifiers.

## Future: Tree-sitter Tokenizer (#70)

For advanced use cases, a tree-sitter based tokenizer can provide:
- Full AST parsing per language
- Distinguish between identifiers, keywords, strings, comments
- Language-specific token weighting

This is tracked in issue #70 and depends on code-simple being implemented first.

---
*From brainstorming session 2026-02-04*
