//! Standalone identifier splitting for code tokens.
//!
//! Splits camelCase, PascalCase, snake_case, kebab-case identifiers
//! into component words, lowercased.

/// Split a code identifier into lowercase component parts.
///
/// Handles camelCase, PascalCase, snake_case, kebab-case, and digit boundaries.
///
/// # Examples
/// ```rust,ignore
/// assert_eq!(split_identifier("getUserById"), vec!["get", "user", "by", "id"]);
/// assert_eq!(split_identifier("XMLParser"), vec!["xml", "parser"]);
/// assert_eq!(split_identifier("snake_case_var"), vec!["snake", "case", "var"]);
/// ```
pub fn split_identifier(ident: &str) -> Vec<String> {
    if ident.is_empty() {
        return vec![];
    }

    let mut parts = Vec::new();
    let chars: Vec<char> = ident.chars().collect();
    let mut current_start = 0;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let prev = if i > 0 { Some(chars[i - 1]) } else { None };
        let next = if i + 1 < chars.len() {
            Some(chars[i + 1])
        } else {
            None
        };

        // Split on underscores and hyphens
        if c == '_' || c == '-' {
            if current_start < i {
                let part: String = chars[current_start..i].iter().collect();
                if !part.is_empty() {
                    parts.push(part.to_lowercase());
                }
            }
            current_start = i + 1;
            i += 1;
            continue;
        }

        let mut should_split = false;

        // Digit boundaries
        if let Some(p) = prev {
            if (p.is_alphabetic() && c.is_ascii_digit())
                || (p.is_ascii_digit() && c.is_alphabetic())
            {
                should_split = true;
            }
        }

        // camelCase boundaries
        if let Some(p) = prev {
            // lowercase -> uppercase
            if p.is_lowercase() && c.is_uppercase() {
                should_split = true;
            }
            // Acronym end: UPPERLower -> split before last upper
            if p.is_uppercase() && c.is_uppercase() {
                if let Some(n) = next {
                    if n.is_lowercase() {
                        should_split = true;
                    }
                }
            }
        }

        if should_split && current_start < i {
            let part: String = chars[current_start..i].iter().collect();
            if !part.is_empty() {
                parts.push(part.to_lowercase());
            }
            current_start = i;
        }

        i += 1;
    }

    // Last part
    if current_start < chars.len() {
        let part: String = chars[current_start..].iter().collect();
        if !part.is_empty() {
            parts.push(part.to_lowercase());
        }
    }

    parts
}

/// Tokenize a string of text (comment or string literal content) into words.
///
/// Simple whitespace + punctuation splitting with lowercasing.
/// Filters out very short tokens (< 2 chars).
pub fn tokenize_text(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| s.len() >= 2)
        .map(|s| s.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_case() {
        assert_eq!(
            split_identifier("getUserById"),
            vec!["get", "user", "by", "id"]
        );
    }

    #[test]
    fn test_pascal_case() {
        assert_eq!(split_identifier("UserService"), vec!["user", "service"]);
    }

    #[test]
    fn test_acronym() {
        assert_eq!(split_identifier("XMLParser"), vec!["xml", "parser"]);
        assert_eq!(split_identifier("parseJSON"), vec!["parse", "json"]);
        assert_eq!(split_identifier("HTMLToJSON"), vec!["html", "to", "json"]);
    }

    #[test]
    fn test_snake_case() {
        assert_eq!(
            split_identifier("get_user_by_id"),
            vec!["get", "user", "by", "id"]
        );
        assert_eq!(split_identifier("__init__"), vec!["init"]);
    }

    #[test]
    fn test_kebab_case() {
        assert_eq!(split_identifier("my-component"), vec!["my", "component"]);
    }

    #[test]
    fn test_digits() {
        assert_eq!(split_identifier("user123"), vec!["user", "123"]);
        assert_eq!(
            split_identifier("get2ndPlace"),
            vec!["get", "2", "nd", "place"]
        );
    }

    #[test]
    fn test_empty() {
        assert_eq!(split_identifier(""), Vec::<String>::new());
    }

    #[test]
    fn test_single() {
        assert_eq!(split_identifier("hello"), vec!["hello"]);
    }

    #[test]
    fn test_tokenize_text() {
        assert_eq!(tokenize_text("This is a test"), vec!["this", "is", "test"]);
        assert_eq!(
            tokenize_text("TODO: fix the bug"),
            vec!["todo", "fix", "the", "bug"]
        );
    }
}
