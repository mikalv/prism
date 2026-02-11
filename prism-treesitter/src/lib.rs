//! Tree-sitter based code tokenizer for Prism
//!
//! Provides AST-aware tokenization of source code for search indexing.
//! Tree-sitter parses code into an AST, then identifiers, comments, and
//! strings are extracted and split using camelCase/snake_case heuristics.
//!
//! # Usage
//!
//! ```rust,ignore
//! use prism_treesitter::register_tokenizers;
//!
//! // Register all enabled language tokenizers with a Tantivy index
//! register_tokenizers(index.tokenizers());
//! ```

mod detector;
mod splitter;
mod tokenizer;

pub use detector::{language_from_content, language_from_extension};
pub use tokenizer::TreeSitterTokenizer;

use tantivy::tokenizer::TokenizerManager;

/// Supported languages for tree-sitter parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    #[cfg(feature = "rust")]
    Rust,
    #[cfg(feature = "python")]
    Python,
    #[cfg(feature = "javascript")]
    JavaScript,
    #[cfg(feature = "typescript")]
    TypeScript,
    #[cfg(feature = "go")]
    Go,
    #[cfg(feature = "c")]
    C,
    #[cfg(feature = "cpp")]
    Cpp,
    #[cfg(feature = "ruby")]
    Ruby,
    #[cfg(feature = "elixir")]
    Elixir,
    #[cfg(feature = "erlang")]
    Erlang,
    #[cfg(feature = "bash")]
    Bash,
    #[cfg(feature = "yaml")]
    Yaml,
    #[cfg(feature = "toml")]
    Toml,
    #[cfg(feature = "json")]
    Json,
    #[cfg(feature = "html")]
    Html,
    #[cfg(feature = "sql")]
    Sql,
}

impl Language {
    /// Get the tree-sitter language for this language variant
    pub fn ts_language(&self) -> tree_sitter::Language {
        match self {
            #[cfg(feature = "rust")]
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            #[cfg(feature = "python")]
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            #[cfg(feature = "javascript")]
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            #[cfg(feature = "typescript")]
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            #[cfg(feature = "go")]
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            #[cfg(feature = "c")]
            Language::C => tree_sitter_c::LANGUAGE.into(),
            #[cfg(feature = "cpp")]
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            #[cfg(feature = "ruby")]
            Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            #[cfg(feature = "elixir")]
            Language::Elixir => tree_sitter_elixir::LANGUAGE.into(),
            #[cfg(feature = "erlang")]
            Language::Erlang => tree_sitter_erlang::LANGUAGE.into(),
            #[cfg(feature = "bash")]
            Language::Bash => tree_sitter_bash::LANGUAGE.into(),
            #[cfg(feature = "yaml")]
            Language::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            #[cfg(feature = "toml")]
            Language::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            #[cfg(feature = "json")]
            Language::Json => tree_sitter_json::LANGUAGE.into(),
            #[cfg(feature = "html")]
            Language::Html => tree_sitter_html::LANGUAGE.into(),
            #[cfg(feature = "sql")]
            Language::Sql => tree_sitter_sequel::LANGUAGE.into(),
        }
    }

    /// Get the tokenizer name suffix for this language
    pub fn name(&self) -> &'static str {
        match self {
            #[cfg(feature = "rust")]
            Language::Rust => "rust",
            #[cfg(feature = "python")]
            Language::Python => "python",
            #[cfg(feature = "javascript")]
            Language::JavaScript => "javascript",
            #[cfg(feature = "typescript")]
            Language::TypeScript => "typescript",
            #[cfg(feature = "go")]
            Language::Go => "go",
            #[cfg(feature = "c")]
            Language::C => "c",
            #[cfg(feature = "cpp")]
            Language::Cpp => "cpp",
            #[cfg(feature = "ruby")]
            Language::Ruby => "ruby",
            #[cfg(feature = "elixir")]
            Language::Elixir => "elixir",
            #[cfg(feature = "erlang")]
            Language::Erlang => "erlang",
            #[cfg(feature = "bash")]
            Language::Bash => "bash",
            #[cfg(feature = "yaml")]
            Language::Yaml => "yaml",
            #[cfg(feature = "toml")]
            Language::Toml => "toml",
            #[cfg(feature = "json")]
            Language::Json => "json",
            #[cfg(feature = "html")]
            Language::Html => "html",
            #[cfg(feature = "sql")]
            Language::Sql => "sql",
        }
    }

    /// Return all enabled language variants
    fn all() -> Vec<Language> {
        vec![
            #[cfg(feature = "rust")]
            Language::Rust,
            #[cfg(feature = "python")]
            Language::Python,
            #[cfg(feature = "javascript")]
            Language::JavaScript,
            #[cfg(feature = "typescript")]
            Language::TypeScript,
            #[cfg(feature = "go")]
            Language::Go,
            #[cfg(feature = "c")]
            Language::C,
            #[cfg(feature = "cpp")]
            Language::Cpp,
            #[cfg(feature = "ruby")]
            Language::Ruby,
            #[cfg(feature = "elixir")]
            Language::Elixir,
            #[cfg(feature = "erlang")]
            Language::Erlang,
            #[cfg(feature = "bash")]
            Language::Bash,
            #[cfg(feature = "yaml")]
            Language::Yaml,
            #[cfg(feature = "toml")]
            Language::Toml,
            #[cfg(feature = "json")]
            Language::Json,
            #[cfg(feature = "html")]
            Language::Html,
            #[cfg(feature = "sql")]
            Language::Sql,
        ]
    }
}

/// Register all tree-sitter tokenizers with a Tantivy TokenizerManager.
///
/// Registers:
/// - `code-treesitter` — auto-detect language from content
/// - `code-treesitter-rust`, `code-treesitter-python`, etc. — explicit language
pub fn register_tokenizers(manager: &TokenizerManager) {
    // Auto-detect tokenizer
    manager.register("code-treesitter", TreeSitterTokenizer::auto_detect());

    // Per-language tokenizers
    for lang in Language::all() {
        let name = format!("code-treesitter-{}", lang.name());
        manager.register(&name, TreeSitterTokenizer::new(lang));
    }
}
