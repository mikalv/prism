//! Code identifier splitting filter
//!
//! Splits code identifiers like camelCase, PascalCase, snake_case into component words.

use tantivy::tokenizer::{Token, TokenFilter, TokenStream, Tokenizer};

/// Configuration for identifier splitting behavior
#[derive(Clone, Debug)]
pub struct CodeIdentifierSplitter {
    /// Split camelCase and PascalCase (e.g., "getUserById" -> ["get", "User", "By", "Id"])
    pub split_camel_case: bool,
    /// Split snake_case (e.g., "get_user" -> ["get", "user"])
    pub split_snake_case: bool,
    /// Split on digits (e.g., "user123" -> ["user", "123"])
    pub split_on_digits: bool,
    /// Minimum token length to emit (shorter tokens are dropped)
    pub min_token_len: usize,
}

impl Default for CodeIdentifierSplitter {
    fn default() -> Self {
        Self {
            split_camel_case: true,
            split_snake_case: true,
            split_on_digits: true,
            min_token_len: 1,
        }
    }
}

impl TokenFilter for CodeIdentifierSplitter {
    type Tokenizer<T: Tokenizer> = CodeIdentifierSplitterFilter<T>;

    fn transform<T: Tokenizer>(self, tokenizer: T) -> Self::Tokenizer<T> {
        CodeIdentifierSplitterFilter {
            config: self,
            inner: tokenizer,
            parts: Vec::new(),
        }
    }
}

/// The tokenizer filter that wraps an inner tokenizer
#[derive(Clone)]
pub struct CodeIdentifierSplitterFilter<T> {
    config: CodeIdentifierSplitter,
    inner: T,
    parts: Vec<Token>,
}

impl<T: Tokenizer> Tokenizer for CodeIdentifierSplitterFilter<T> {
    type TokenStream<'a> = CodeIdentifierSplitterStream<'a, T::TokenStream<'a>>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        self.parts.clear();
        CodeIdentifierSplitterStream {
            config: &self.config,
            tail: self.inner.token_stream(text),
            parts: &mut self.parts,
        }
    }
}

/// The token stream that performs the actual splitting
pub struct CodeIdentifierSplitterStream<'a, T> {
    config: &'a CodeIdentifierSplitter,
    tail: T,
    parts: &'a mut Vec<Token>,
}

impl<'a, T: TokenStream> CodeIdentifierSplitterStream<'a, T> {
    /// Split a token into parts based on identifier conventions
    fn split_token(&mut self) {
        let token = self.tail.token();
        let text = &token.text;

        if text.is_empty() {
            return;
        }

        let mut parts = Vec::new();
        let mut current_start = 0;
        let chars: Vec<char> = text.chars().collect();

        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let next = if i + 1 < chars.len() {
                Some(chars[i + 1])
            } else {
                None
            };

            let mut should_split = false;

            // Split on underscores and hyphens (snake_case, kebab-case)
            if self.config.split_snake_case && (c == '_' || c == '-') {
                if current_start < i {
                    let part: String = chars[current_start..i].iter().collect();
                    if part.len() >= self.config.min_token_len {
                        parts.push(part);
                    }
                }
                current_start = i + 1;
                i += 1;
                continue;
            }

            // Split on digit boundaries
            if self.config.split_on_digits {
                if let Some(p) = prev {
                    // Transition from letter to digit or digit to letter
                    if (p.is_alphabetic() && c.is_ascii_digit())
                        || (p.is_ascii_digit() && c.is_alphabetic())
                    {
                        should_split = true;
                    }
                }
            }

            // Split on camelCase boundaries
            if self.config.split_camel_case {
                if let Some(p) = prev {
                    // lowercase followed by uppercase: "getUser" -> split before 'U'
                    if p.is_lowercase() && c.is_uppercase() {
                        should_split = true;
                    }

                    // Sequence of uppercase followed by lowercase: "XMLParser" -> split before 'P'
                    // This handles acronyms: "HTTPSConnection" -> "HTTPS" + "Connection"
                    if p.is_uppercase() && c.is_uppercase() {
                        if let Some(n) = next {
                            if n.is_lowercase() {
                                should_split = true;
                            }
                        }
                    }
                }
            }

            if should_split && current_start < i {
                let part: String = chars[current_start..i].iter().collect();
                if part.len() >= self.config.min_token_len {
                    parts.push(part);
                }
                current_start = i;
            }

            i += 1;
        }

        // Don't forget the last part
        if current_start < chars.len() {
            let part: String = chars[current_start..].iter().collect();
            if part.len() >= self.config.min_token_len {
                parts.push(part);
            }
        }

        // If we got multiple parts, add them in reverse order so pop() gives correct order
        if parts.len() > 1 {
            for part in parts.into_iter().rev() {
                self.parts.push(Token {
                    text: part,
                    ..*token
                });
            }
        } else if parts.len() == 1 {
            // Single part - just use original token
            // (will fall through to tail.token())
        }
        // If no parts, original token will be used
    }
}

impl<'a, T: TokenStream> TokenStream for CodeIdentifierSplitterStream<'a, T> {
    fn advance(&mut self) -> bool {
        // First, try to return buffered parts
        self.parts.pop();

        if !self.parts.is_empty() {
            return true;
        }

        // Get next token from underlying stream
        if !self.tail.advance() {
            return false;
        }

        // Try to split it
        self.split_token();
        true
    }

    fn token(&self) -> &Token {
        self.parts.last().unwrap_or_else(|| self.tail.token())
    }

    fn token_mut(&mut self) -> &mut Token {
        self.parts
            .last_mut()
            .unwrap_or_else(|| self.tail.token_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::tokenizer::{SimpleTokenizer, TextAnalyzer};

    fn split(text: &str) -> Vec<String> {
        let mut analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(CodeIdentifierSplitter::default())
            .build();

        let mut stream = analyzer.token_stream(text);
        let mut tokens = Vec::new();
        let mut add_token = |token: &Token| {
            tokens.push(token.text.clone());
        };
        stream.process(&mut add_token);
        tokens
    }

    #[test]
    fn test_simple_word() {
        assert_eq!(split("hello"), vec!["hello"]);
    }

    #[test]
    fn test_camel_case() {
        assert_eq!(split("getUserById"), vec!["get", "User", "By", "Id"]);
        assert_eq!(split("isHTTPSEnabled"), vec!["is", "HTTPS", "Enabled"]);
    }

    #[test]
    fn test_pascal_case() {
        assert_eq!(split("UserService"), vec!["User", "Service"]);
        assert_eq!(split("HTTPConnection"), vec!["HTTP", "Connection"]);
    }

    #[test]
    fn test_snake_case() {
        assert_eq!(split("get_user_by_id"), vec!["get", "user", "by", "id"]);
        assert_eq!(split("__init__"), vec!["init"]);
    }

    #[test]
    fn test_kebab_case() {
        assert_eq!(split("my-component"), vec!["my", "component"]);
    }

    #[test]
    fn test_with_digits() {
        assert_eq!(split("user123"), vec!["user", "123"]);
        assert_eq!(split("123user"), vec!["123", "user"]);
        assert_eq!(split("get2ndPlace"), vec!["get", "2", "nd", "Place"]);
    }

    #[test]
    fn test_mixed() {
        assert_eq!(split("myFunc_name123"), vec!["my", "Func", "name", "123"]);
    }

    #[test]
    fn test_multiple_words() {
        assert_eq!(split("hello world"), vec!["hello", "world"]);
        assert_eq!(
            split("getUserById fetchData"),
            vec!["get", "User", "By", "Id", "fetch", "Data"]
        );
    }

    #[test]
    fn test_acronyms() {
        assert_eq!(split("XMLParser"), vec!["XML", "Parser"]);
        assert_eq!(split("parseXML"), vec!["parse", "XML"]);
        assert_eq!(split("HTMLToJSON"), vec!["HTML", "To", "JSON"]);
    }
}
