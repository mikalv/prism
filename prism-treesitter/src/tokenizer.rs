//! TreeSitterTokenizer — a Tantivy Tokenizer backed by tree-sitter parsing.
//!
//! Parses source code into an AST, walks nodes, and emits tokens for
//! identifiers (split by camelCase/snake_case), comments, strings, and keywords.

use crate::detector::language_from_content;
use crate::splitter::{split_identifier, tokenize_text};
use crate::Language;
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

/// Tree-sitter based code tokenizer.
///
/// Implements the Tantivy `Tokenizer` trait. When language is `None`,
/// auto-detection is attempted from the content.
#[derive(Clone)]
pub struct TreeSitterTokenizer {
    language: Option<Language>,
    index_comments: bool,
    index_strings: bool,
}

impl TreeSitterTokenizer {
    /// Create a tokenizer for a specific language.
    pub fn new(language: Language) -> Self {
        Self {
            language: Some(language),
            index_comments: true,
            index_strings: true,
        }
    }

    /// Create an auto-detecting tokenizer.
    pub fn auto_detect() -> Self {
        Self {
            language: None,
            index_comments: true,
            index_strings: true,
        }
    }
}

impl Tokenizer for TreeSitterTokenizer {
    type TokenStream<'a> = TreeSitterTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let lang = self.language.or_else(|| language_from_content(text));

        let tokens = match lang {
            Some(lang) => parse_and_extract(text, lang, self.index_comments, self.index_strings),
            None => fallback_tokenize(text),
        };

        TreeSitterTokenStream {
            tokens,
            index: 0,
            token: Token::default(),
        }
    }
}

/// Token stream produced by TreeSitterTokenizer.
pub struct TreeSitterTokenStream {
    tokens: Vec<ExtractedToken>,
    index: usize,
    token: Token,
}

#[derive(Debug)]
struct ExtractedToken {
    text: String,
    offset_from: usize,
    offset_to: usize,
}

impl TokenStream for TreeSitterTokenStream {
    fn advance(&mut self) -> bool {
        if self.index >= self.tokens.len() {
            return false;
        }
        let t = &self.tokens[self.index];
        self.token = Token {
            offset_from: t.offset_from,
            offset_to: t.offset_to,
            position: self.index,
            text: t.text.clone(),
            position_length: 1,
        };
        self.index += 1;
        true
    }

    fn token(&self) -> &Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}

/// Check if a tree-sitter node kind represents an identifier.
fn is_identifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "shorthand_property_identifier"
            | "shorthand_property_identifier_pattern"
            | "attribute_item"
            | "constant"
            | "alias"
            | "atom"
            | "variable"
            | "variable_name"
            | "name"
            | "simple_identifier"
    )
}

/// Check if a tree-sitter node kind represents a string literal.
fn is_string_kind(kind: &str) -> bool {
    matches!(
        kind,
        "string_literal"
            | "string"
            | "string_content"
            | "template_string"
            | "interpreted_string_literal"
            | "raw_string_literal"
            | "heredoc_body"
    )
}

/// Check if a tree-sitter node kind represents a comment.
fn is_comment_kind(kind: &str) -> bool {
    matches!(
        kind,
        "line_comment" | "block_comment" | "comment"
    )
}

/// Parse text with tree-sitter and extract tokens.
fn parse_and_extract(
    text: &str,
    lang: Language,
    index_comments: bool,
    index_strings: bool,
) -> Vec<ExtractedToken> {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&lang.ts_language()).is_err() {
        return fallback_tokenize(text);
    }

    let tree = match parser.parse(text, None) {
        Some(tree) => tree,
        None => return fallback_tokenize(text),
    };

    let mut tokens = Vec::new();
    let mut cursor = tree.walk();
    walk_tree(
        &mut cursor,
        text,
        &mut tokens,
        index_comments,
        index_strings,
    );

    tokens
}

/// Recursively walk the tree-sitter AST and extract tokens.
fn walk_tree(
    cursor: &mut tree_sitter::TreeCursor,
    text: &str,
    tokens: &mut Vec<ExtractedToken>,
    index_comments: bool,
    index_strings: bool,
) {
    loop {
        let node = cursor.node();
        let kind = node.kind();
        let start = node.start_byte();
        let end = node.end_byte();

        if end <= text.len() {
            let node_text = &text[start..end];

            if is_identifier_kind(kind) {
                // Split the identifier and emit sub-tokens
                let parts = split_identifier(node_text);
                for part in parts {
                    if !part.is_empty() {
                        tokens.push(ExtractedToken {
                            text: part,
                            offset_from: start,
                            offset_to: end,
                        });
                    }
                }
                // Don't recurse into identifier children
                if !cursor.goto_next_sibling() {
                    loop {
                        if !cursor.goto_parent() {
                            return;
                        }
                        if cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
                continue;
            }

            if is_comment_kind(kind) && index_comments {
                // Strip comment markers and tokenize content
                let content = strip_comment_markers(node_text);
                let words = tokenize_text(&content);
                for word in words {
                    tokens.push(ExtractedToken {
                        text: word,
                        offset_from: start,
                        offset_to: end,
                    });
                }
                // Don't recurse into comment children
                if !cursor.goto_next_sibling() {
                    loop {
                        if !cursor.goto_parent() {
                            return;
                        }
                        if cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
                continue;
            }

            if is_string_kind(kind) && index_strings {
                // Strip quotes and tokenize content
                let content = strip_string_delimiters(node_text);
                let words = tokenize_text(&content);
                for word in words {
                    tokens.push(ExtractedToken {
                        text: word,
                        offset_from: start,
                        offset_to: end,
                    });
                }
                // Don't recurse into string children
                if !cursor.goto_next_sibling() {
                    loop {
                        if !cursor.goto_parent() {
                            return;
                        }
                        if cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
                continue;
            }
        }

        // Try children first
        if cursor.goto_first_child() {
            continue;
        }

        // Try next sibling
        if cursor.goto_next_sibling() {
            continue;
        }

        // Go up until we can go to next sibling
        loop {
            if !cursor.goto_parent() {
                return;
            }
            if cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Strip comment markers (// /* */ # --) from comment text.
fn strip_comment_markers(text: &str) -> String {
    let text = text.trim();
    if let Some(stripped) = text.strip_prefix("//") {
        return stripped.trim_start_matches('/').trim().to_string();
    }
    if text.starts_with("/*") && text.ends_with("*/") {
        let inner = &text[2..text.len() - 2];
        // Strip leading * from each line (doc comments)
        return inner
            .lines()
            .map(|line| line.trim().trim_start_matches('*').trim())
            .collect::<Vec<_>>()
            .join(" ");
    }
    if let Some(stripped) = text.strip_prefix('#') {
        return stripped.trim().to_string();
    }
    if let Some(stripped) = text.strip_prefix("--") {
        return stripped.trim().to_string();
    }
    text.to_string()
}

/// Strip string delimiters (quotes) from string literal text.
fn strip_string_delimiters(text: &str) -> String {
    let text = text.trim();
    // Triple quotes
    if (text.starts_with("\"\"\"") && text.ends_with("\"\"\"") && text.len() >= 6)
        || (text.starts_with("'''") && text.ends_with("'''") && text.len() >= 6)
    {
        return text[3..text.len() - 3].to_string();
    }
    // Regular quotes
    if (text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\''))
        || (text.starts_with('`') && text.ends_with('`'))
    {
        if text.len() >= 2 {
            return text[1..text.len() - 1].to_string();
        }
    }
    text.to_string()
}

/// Fallback tokenization when tree-sitter parsing is unavailable.
/// Simple whitespace + punctuation splitting with identifier sub-splitting.
fn fallback_tokenize(text: &str) -> Vec<ExtractedToken> {
    let mut tokens = Vec::new();
    let mut offset = 0;

    for word in text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '{' | '}' | '[' | ']' | ';' | ',' | '.' | ':' | '<' | '>' | '=' | '+' | '-' | '*' | '/' | '&' | '|' | '!' | '?' | '@' | '#' | '$' | '%' | '^' | '~')) {
        if !word.is_empty() && word.len() >= 2 {
            let parts = split_identifier(word);
            for part in parts {
                if !part.is_empty() {
                    tokens.push(ExtractedToken {
                        text: part,
                        offset_from: offset,
                        offset_to: offset + word.len(),
                    });
                }
            }
        }
        offset += word.len() + 1; // +1 for the delimiter
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize_with_lang(text: &str, lang: Language) -> Vec<String> {
        let mut tokenizer = TreeSitterTokenizer::new(lang);
        let mut stream = tokenizer.token_stream(text);
        let mut result = Vec::new();
        while stream.advance() {
            result.push(stream.token().text.clone());
        }
        result
    }

    fn tokenize_auto(text: &str) -> Vec<String> {
        let mut tokenizer = TreeSitterTokenizer::auto_detect();
        let mut stream = tokenizer.token_stream(text);
        let mut result = Vec::new();
        while stream.advance() {
            result.push(stream.token().text.clone());
        }
        result
    }

    #[cfg(feature = "rust")]
    #[test]
    fn test_rust_function() {
        let code = r#"fn getUserName(user_id: u64) -> String {
    let userName = "default";
    userName.to_string()
}"#;
        let tokens = tokenize_with_lang(code, Language::Rust);
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"name".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"id".to_string()));
        // String content
        assert!(tokens.contains(&"default".to_string()));
    }

    #[cfg(feature = "rust")]
    #[test]
    fn test_rust_comment() {
        let code = r#"
// This function processes data
fn process_data() {}
"#;
        let tokens = tokenize_with_lang(code, Language::Rust);
        assert!(tokens.contains(&"this".to_string()));
        assert!(tokens.contains(&"function".to_string()));
        assert!(tokens.contains(&"processes".to_string()));
        assert!(tokens.contains(&"data".to_string()));
        assert!(tokens.contains(&"process".to_string()));
    }

    #[cfg(feature = "python")]
    #[test]
    fn test_python_function() {
        let code = r#"
def get_user_name(user_id):
    """Get the user name by ID"""
    return user_name
"#;
        let tokens = tokenize_with_lang(code, Language::Python);
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"name".to_string()));
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn test_javascript_function() {
        let code = r#"
function getUserById(userId) {
    // Fetch from database
    const userName = "test";
    return userName;
}
"#;
        let tokens = tokenize_with_lang(code, Language::JavaScript);
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"by".to_string()));
        assert!(tokens.contains(&"id".to_string()));
    }

    #[cfg(feature = "go")]
    #[test]
    fn test_go_function() {
        let code = r#"
package main

func GetUserName(userId int) string {
    return "hello"
}
"#;
        let tokens = tokenize_with_lang(code, Language::Go);
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"name".to_string()));
    }

    #[cfg(feature = "sql")]
    #[test]
    fn test_sql_query() {
        let code = "SELECT user_name, email FROM users WHERE active = true;";
        let tokens = tokenize_with_lang(code, Language::Sql);
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"name".to_string()));
        assert!(tokens.contains(&"email".to_string()));
        assert!(tokens.contains(&"users".to_string()));
    }

    #[test]
    fn test_fallback_tokenize() {
        let tokens = tokenize_auto("some random text without code markers");
        assert!(tokens.contains(&"some".to_string()));
        assert!(tokens.contains(&"random".to_string()));
        assert!(tokens.contains(&"text".to_string()));
    }

    #[test]
    fn test_strip_comment_markers() {
        assert_eq!(strip_comment_markers("// hello world"), "hello world");
        assert_eq!(strip_comment_markers("/* block */"), "block");
        assert_eq!(strip_comment_markers("# python comment"), "python comment");
    }

    #[test]
    fn test_strip_string_delimiters() {
        assert_eq!(strip_string_delimiters("\"hello\""), "hello");
        assert_eq!(strip_string_delimiters("'world'"), "world");
        assert_eq!(strip_string_delimiters("`template`"), "template");
    }
}
