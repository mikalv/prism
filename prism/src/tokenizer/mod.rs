//! Code-aware tokenizers for Prism
//!
//! This module provides tokenizers optimized for code search, handling:
//! - camelCase and PascalCase identifier splitting
//! - snake_case and kebab-case splitting
//! - Operator preservation
//!
//! # Example
//!
//! ```rust,ignore
//! use prism::tokenizer::CodeTokenizer;
//! use tantivy::tokenizer::TextAnalyzer;
//!
//! let tokenizer = CodeTokenizer::default();
//! // "getUserById" -> ["get", "user", "by", "id"]
//! // "snake_case_var" -> ["snake", "case", "var"]
//! ```

mod splitter;

pub use splitter::{CodeIdentifierSplitter, CodeIdentifierSplitterFilter};

use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer};

/// Tokenizer name for code-aware tokenization
pub const CODE_TOKENIZER_NAME: &str = "code";

/// Create the default code tokenizer
///
/// This tokenizer:
/// 1. Splits on whitespace and punctuation (SimpleTokenizer)
/// 2. Splits camelCase, PascalCase, snake_case, kebab-case identifiers
/// 3. Lowercases all tokens
/// 4. Removes tokens longer than 100 characters
pub fn code_tokenizer() -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(CodeIdentifierSplitter::default())
        .filter(LowerCaser)
        .filter(RemoveLongFilter::limit(100))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::tokenizer::Token;

    fn tokenize(text: &str) -> Vec<String> {
        let mut analyzer = code_tokenizer();
        let mut stream = analyzer.token_stream(text);
        let mut tokens = Vec::new();
        let mut add_token = |token: &Token| {
            tokens.push(token.text.clone());
        };
        stream.process(&mut add_token);
        tokens
    }

    #[test]
    fn test_camel_case() {
        assert_eq!(tokenize("getUserById"), vec!["get", "user", "by", "id"]);
        assert_eq!(tokenize("parseJSON"), vec!["parse", "json"]);
        assert_eq!(tokenize("XMLParser"), vec!["xml", "parser"]);
    }

    #[test]
    fn test_pascal_case() {
        assert_eq!(tokenize("UserService"), vec!["user", "service"]);
        assert_eq!(tokenize("HTTPSConnection"), vec!["https", "connection"]);
    }

    #[test]
    fn test_snake_case() {
        assert_eq!(tokenize("get_user_by_id"), vec!["get", "user", "by", "id"]);
        assert_eq!(tokenize("__private_var__"), vec!["private", "var"]);
    }

    #[test]
    fn test_kebab_case() {
        assert_eq!(
            tokenize("my-component-name"),
            vec!["my", "component", "name"]
        );
    }

    #[test]
    fn test_mixed_text() {
        assert_eq!(
            tokenize("The getUserById function returns a User object"),
            vec!["the", "get", "user", "by", "id", "function", "returns", "a", "user", "object"]
        );
    }

    #[test]
    fn test_numbers() {
        assert_eq!(tokenize("user123"), vec!["user", "123"]);
        assert_eq!(tokenize("get2ndUser"), vec!["get", "2", "nd", "user"]);
    }
}
