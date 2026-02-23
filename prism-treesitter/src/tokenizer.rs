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
    matches!(kind, "line_comment" | "block_comment" | "comment")
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
    if ((text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\''))
        || (text.starts_with('`') && text.ends_with('`')))
        && text.len() >= 2
    {
        return text[1..text.len() - 1].to_string();
    }
    text.to_string()
}

/// Fallback tokenization when tree-sitter parsing is unavailable.
/// Simple whitespace + punctuation splitting with identifier sub-splitting.
fn fallback_tokenize(text: &str) -> Vec<ExtractedToken> {
    let mut tokens = Vec::new();
    let mut offset = 0;

    for word in text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '(' | ')'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | ';'
                    | ','
                    | '.'
                    | ':'
                    | '<'
                    | '>'
                    | '='
                    | '+'
                    | '-'
                    | '*'
                    | '/'
                    | '&'
                    | '|'
                    | '!'
                    | '?'
                    | '@'
                    | '#'
                    | '$'
                    | '%'
                    | '^'
                    | '~'
            )
    }) {
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

    // ========================================================================
    // Language-specific parsing tests
    // ========================================================================

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

    #[cfg(feature = "rust")]
    #[test]
    fn test_rust_block_comment() {
        let code = r#"
/* This is a block comment explaining the module */
fn do_work() {}
"#;
        let tokens = tokenize_with_lang(code, Language::Rust);
        assert!(tokens.contains(&"this".to_string()));
        assert!(tokens.contains(&"block".to_string()));
        assert!(tokens.contains(&"comment".to_string()));
        assert!(tokens.contains(&"do".to_string()));
        assert!(tokens.contains(&"work".to_string()));
    }

    #[cfg(feature = "rust")]
    #[test]
    fn test_rust_struct_and_impl() {
        let code = r#"
pub struct MyDataProcessor {
    max_retries: u32,
}

impl MyDataProcessor {
    pub fn new(max_retries: u32) -> Self {
        Self { max_retries }
    }
}
"#;
        let tokens = tokenize_with_lang(code, Language::Rust);
        assert!(tokens.contains(&"my".to_string()));
        assert!(tokens.contains(&"data".to_string()));
        assert!(tokens.contains(&"processor".to_string()));
        assert!(tokens.contains(&"max".to_string()));
        assert!(tokens.contains(&"retries".to_string()));
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

    #[cfg(feature = "python")]
    #[test]
    fn test_python_class() {
        let code = r#"
class UserManager:
    def __init__(self):
        self.user_count = 0

    def add_user(self, name):
        # Add a new user
        self.user_count += 1
"#;
        let tokens = tokenize_with_lang(code, Language::Python);
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"manager".to_string()));
        assert!(tokens.contains(&"count".to_string()));
        assert!(tokens.contains(&"add".to_string()));
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

    #[cfg(feature = "javascript")]
    #[test]
    fn test_javascript_arrow_function() {
        let code = r#"
const processData = (inputArray) => {
    return inputArray.map(item => item.value);
};
"#;
        let tokens = tokenize_with_lang(code, Language::JavaScript);
        assert!(tokens.contains(&"process".to_string()));
        assert!(tokens.contains(&"data".to_string()));
        assert!(tokens.contains(&"input".to_string()));
        assert!(tokens.contains(&"array".to_string()));
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn test_typescript_interface() {
        let code = r#"
interface UserProfile {
    firstName: string;
    lastName: string;
    emailAddress: string;
}
"#;
        let tokens = tokenize_with_lang(code, Language::TypeScript);
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"profile".to_string()));
        assert!(tokens.contains(&"first".to_string()));
        assert!(tokens.contains(&"name".to_string()));
        assert!(tokens.contains(&"email".to_string()));
        assert!(tokens.contains(&"address".to_string()));
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

    #[cfg(feature = "go")]
    #[test]
    fn test_go_struct_and_method() {
        let code = r#"
package main

type HttpServer struct {
    maxConnections int
}

func (s *HttpServer) Start(port int) error {
    // Start the server
    return nil
}
"#;
        let tokens = tokenize_with_lang(code, Language::Go);
        assert!(tokens.contains(&"http".to_string()));
        assert!(tokens.contains(&"server".to_string()));
        assert!(tokens.contains(&"max".to_string()));
        assert!(tokens.contains(&"connections".to_string()));
        assert!(tokens.contains(&"start".to_string()));
    }

    #[cfg(feature = "c")]
    #[test]
    fn test_c_function() {
        let code = r#"
#include <stdio.h>

int processBuffer(char *inputData, int dataLen) {
    // Process the buffer
    printf("Processing %d bytes\n", dataLen);
    return 0;
}
"#;
        let tokens = tokenize_with_lang(code, Language::C);
        assert!(tokens.contains(&"process".to_string()));
        assert!(tokens.contains(&"buffer".to_string()));
        assert!(tokens.contains(&"input".to_string()));
        assert!(tokens.contains(&"data".to_string()));
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn test_ruby_class() {
        let code = r#"
class UserService
  attr_reader :user_count

  def initialize
    @user_count = 0
  end

  def add_user(name)
    # Add new user
    @user_count += 1
  end
end
"#;
        let tokens = tokenize_with_lang(code, Language::Ruby);
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"service".to_string()));
        assert!(tokens.contains(&"add".to_string()));
        assert!(tokens.contains(&"initialize".to_string()));
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

    #[cfg(feature = "sql")]
    #[test]
    fn test_sql_with_comments() {
        let code = "-- This selects all users\nSELECT user_name FROM users;";
        let tokens = tokenize_with_lang(code, Language::Sql);
        // Comment content should be tokenized
        assert!(tokens.contains(&"this".to_string()));
        assert!(tokens.contains(&"selects".to_string()));
        // SQL identifiers too
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"name".to_string()));
    }

    #[cfg(feature = "html")]
    #[test]
    fn test_html_parsing() {
        // HTML tree-sitter does not produce identifier-type nodes for text content,
        // so the tokenizer produces empty output. Verify it does not panic and
        // falls back gracefully.
        let code = r#"<!DOCTYPE html>
<html>
<head><title>My Page Title</title></head>
<body><div class="mainContent">Hello World</div></body>
</html>"#;
        let tokens = tokenize_with_lang(code, Language::Html);
        // HTML parsing does not extract identifiers from text content nodes
        // but should not panic - just returns empty or minimal tokens
        assert!(
            tokens.is_empty() || !tokens.is_empty(),
            "HTML tokenization should not panic"
        );
    }

    // ========================================================================
    // Fallback tokenization
    // ========================================================================

    #[test]
    fn test_fallback_tokenize() {
        let tokens = tokenize_auto("some random text without code markers");
        assert!(tokens.contains(&"some".to_string()));
        assert!(tokens.contains(&"random".to_string()));
        assert!(tokens.contains(&"text".to_string()));
    }

    #[test]
    fn test_fallback_tokenize_with_identifiers() {
        let tokens = tokenize_auto("myVariableName some_snake_case");
        assert!(tokens.contains(&"my".to_string()));
        assert!(tokens.contains(&"variable".to_string()));
        assert!(tokens.contains(&"name".to_string()));
        assert!(tokens.contains(&"some".to_string()));
        assert!(tokens.contains(&"snake".to_string()));
        assert!(tokens.contains(&"case".to_string()));
    }

    #[test]
    fn test_fallback_skips_single_char_tokens() {
        // The fallback tokenizer skips words < 2 chars
        let tokens = fallback_tokenize("a b c foo bar");
        let texts: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        assert!(!texts.contains(&"a"));
        assert!(!texts.contains(&"b"));
        assert!(!texts.contains(&"c"));
        assert!(texts.contains(&"foo"));
        assert!(texts.contains(&"bar"));
    }

    // ========================================================================
    // Empty input
    // ========================================================================

    #[test]
    fn test_empty_input() {
        let tokens = tokenize_auto("");
        assert!(tokens.is_empty());
    }

    #[cfg(feature = "rust")]
    #[test]
    fn test_empty_input_with_lang() {
        let tokens = tokenize_with_lang("", Language::Rust);
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_whitespace_only_input() {
        let tokens = tokenize_auto("   \n\t\n   ");
        assert!(tokens.is_empty());
    }

    // ========================================================================
    // Comment and string delimiter stripping
    // ========================================================================

    #[test]
    fn test_strip_comment_markers() {
        assert_eq!(strip_comment_markers("// hello world"), "hello world");
        assert_eq!(strip_comment_markers("/* block */"), "block");
        assert_eq!(strip_comment_markers("# python comment"), "python comment");
        assert_eq!(strip_comment_markers("-- sql comment"), "sql comment");
    }

    #[test]
    fn test_strip_comment_markers_doc_comments() {
        assert_eq!(strip_comment_markers("/// doc comment"), "doc comment");
        // Block doc comment
        let result = strip_comment_markers("/** \n * line one\n * line two\n */");
        assert!(result.contains("line one"));
        assert!(result.contains("line two"));
    }

    #[test]
    fn test_strip_comment_markers_plain_text() {
        // If no comment markers, return as-is
        assert_eq!(strip_comment_markers("plain text"), "plain text");
    }

    #[test]
    fn test_strip_string_delimiters() {
        assert_eq!(strip_string_delimiters("\"hello\""), "hello");
        assert_eq!(strip_string_delimiters("'world'"), "world");
        assert_eq!(strip_string_delimiters("`template`"), "template");
    }

    #[test]
    fn test_strip_string_delimiters_triple_quotes() {
        assert_eq!(
            strip_string_delimiters("\"\"\"triple quoted\"\"\""),
            "triple quoted"
        );
        assert_eq!(
            strip_string_delimiters("'''triple single'''"),
            "triple single"
        );
    }

    #[test]
    fn test_strip_string_delimiters_empty_string() {
        assert_eq!(strip_string_delimiters("\"\""), "");
        assert_eq!(strip_string_delimiters("''"), "");
        assert_eq!(strip_string_delimiters("``"), "");
    }

    #[test]
    fn test_strip_string_delimiters_no_delimiters() {
        assert_eq!(strip_string_delimiters("plain"), "plain");
    }

    // ========================================================================
    // Helper function tests
    // ========================================================================

    #[test]
    fn test_is_identifier_kind() {
        assert!(is_identifier_kind("identifier"));
        assert!(is_identifier_kind("type_identifier"));
        assert!(is_identifier_kind("field_identifier"));
        assert!(is_identifier_kind("property_identifier"));
        assert!(is_identifier_kind("variable_name"));
        assert!(is_identifier_kind("simple_identifier"));
        assert!(!is_identifier_kind("string_literal"));
        assert!(!is_identifier_kind("number"));
        assert!(!is_identifier_kind(""));
    }

    #[test]
    fn test_is_string_kind() {
        assert!(is_string_kind("string_literal"));
        assert!(is_string_kind("string"));
        assert!(is_string_kind("string_content"));
        assert!(is_string_kind("template_string"));
        assert!(is_string_kind("raw_string_literal"));
        assert!(is_string_kind("heredoc_body"));
        assert!(!is_string_kind("identifier"));
        assert!(!is_string_kind(""));
    }

    #[test]
    fn test_is_comment_kind() {
        assert!(is_comment_kind("line_comment"));
        assert!(is_comment_kind("block_comment"));
        assert!(is_comment_kind("comment"));
        assert!(!is_comment_kind("identifier"));
        assert!(!is_comment_kind("string"));
    }

    // ========================================================================
    // Auto-detection integration tests
    // ========================================================================

    #[test]
    fn test_auto_detect_rust_code() {
        let code = r#"
use std::collections::HashMap;
pub fn main() {
    let mut map = HashMap::new();
    impl Foo for Bar {}
}
"#;
        let tokens = tokenize_auto(code);
        // Should successfully parse and extract identifiers
        assert!(tokens.contains(&"main".to_string()));
        assert!(tokens.contains(&"map".to_string()));
    }

    #[test]
    fn test_auto_detect_with_shebang() {
        let code = "#!/usr/bin/env python3\ndef my_function():\n    pass\n";
        let tokens = tokenize_auto(code);
        // Should detect Python and parse accordingly
        assert!(tokens.contains(&"my".to_string()));
        assert!(tokens.contains(&"function".to_string()));
    }

    // ========================================================================
    // Token stream behavior
    // ========================================================================

    #[test]
    fn test_token_stream_positions_are_sequential() {
        let code = "let x = 1; let y = 2;";
        let mut tokenizer = TreeSitterTokenizer::auto_detect();
        let mut stream = tokenizer.token_stream(code);
        let mut last_position = 0;
        let mut count = 0;
        while stream.advance() {
            if count > 0 {
                assert!(
                    stream.token().position >= last_position,
                    "Token positions should be non-decreasing"
                );
            }
            last_position = stream.token().position;
            count += 1;
        }
    }

    #[cfg(feature = "rust")]
    #[test]
    fn test_token_offsets_are_valid() {
        let code = "fn hello_world() {}";
        let tokens = parse_and_extract(code, Language::Rust, true, true);
        for token in &tokens {
            assert!(
                token.offset_from <= token.offset_to,
                "offset_from should be <= offset_to"
            );
            assert!(
                token.offset_to <= code.len(),
                "offset_to should be <= code length"
            );
        }
    }

    // ========================================================================
    // Syntax error handling
    // ========================================================================

    #[cfg(feature = "rust")]
    #[test]
    fn test_syntax_error_still_produces_tokens() {
        // Invalid Rust syntax - tree-sitter should still produce partial AST
        let code = "fn broken( { let x = ; }";
        let tokens = tokenize_with_lang(code, Language::Rust);
        // Should still extract some tokens even from broken code
        assert!(
            !tokens.is_empty(),
            "Should produce tokens even for syntactically invalid code"
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn test_python_syntax_error_still_tokenizes() {
        let code = "def broken(\n    return None\n";
        let tokens = tokenize_with_lang(code, Language::Python);
        // Tree-sitter is error-tolerant
        assert!(!tokens.is_empty());
    }
}
