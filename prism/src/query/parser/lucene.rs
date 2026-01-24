use super::ast::*;
use super::{QueryError, Result};

pub struct LuceneParser;

impl LuceneParser {
    pub fn parse(query: &str) -> Result<QueryNode> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(QueryError::ParseError("Empty query".to_string()));
        }

        Self::parse_or(trimmed)
    }

    // Precedence: OR (lowest) > AND > NOT > primary (highest)
    fn parse_or(input: &str) -> Result<QueryNode> {
        let parts = Self::split_by_operator(input, " OR ");
        if parts.len() > 1 {
            let children: Result<Vec<_>> = parts.iter().map(|p| Self::parse_and(p)).collect();
            return Ok(QueryNode::Or(children?));
        }
        Self::parse_and(input)
    }

    fn parse_and(input: &str) -> Result<QueryNode> {
        let parts = Self::split_by_operator(input, " AND ");
        if parts.len() > 1 {
            let children: Result<Vec<_>> = parts.iter().map(|p| Self::parse_not(p)).collect();
            return Ok(QueryNode::And(children?));
        }
        Self::parse_not(input)
    }

    fn parse_not(input: &str) -> Result<QueryNode> {
        let trimmed = input.trim();
        if trimmed.starts_with("NOT ") {
            let rest = trimmed.strip_prefix("NOT ").unwrap();
            let child = Self::parse_primary(rest)?;
            return Ok(QueryNode::Not(Box::new(child)));
        }
        Self::parse_primary(input)
    }

    fn parse_primary(input: &str) -> Result<QueryNode> {
        let trimmed = input.trim();

        // Handle parentheses
        if trimmed.starts_with('(') && trimmed.ends_with(')') {
            let inner = &trimmed[1..trimmed.len() - 1];
            return Self::parse_or(inner);
        }

        // Handle boost (term^2.0)
        if let Some((term_part, boost_part)) = trimmed.rsplit_once('^') {
            if let Ok(boost) = boost_part.parse::<f32>() {
                let query = Self::parse_term(term_part)?;
                return Ok(QueryNode::Boost {
                    query: Box::new(query),
                    boost,
                });
            }
        }

        Self::parse_term(trimmed)
    }

    fn parse_term(input: &str) -> Result<QueryNode> {
        let trimmed = input.trim();

        // Handle field:value
        if let Some((field, value)) = trimmed.split_once(':') {
            if !value.contains(' ') {
                return Ok(QueryNode::field_term(field, value));
            }
        }

        // Simple term
        if !trimmed.contains(' ') {
            return Ok(QueryNode::term(trimmed));
        }

        // Multi-word phrase
        let words: Vec<String> = trimmed.split_whitespace().map(|s| s.to_string()).collect();
        Ok(QueryNode::phrase(words))
    }

    // Split by operator, respecting parentheses
    fn split_by_operator(input: &str, operator: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut paren_depth = 0;
        let bytes = input.as_bytes();
        let op_bytes = operator.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] == b'(' {
                paren_depth += 1;
                current.push('(');
                i += 1;
            } else if bytes[i] == b')' {
                paren_depth -= 1;
                current.push(')');
                i += 1;
            } else if paren_depth == 0 && i + op_bytes.len() <= bytes.len() {
                // Check if we're at the operator
                if &bytes[i..i + op_bytes.len()] == op_bytes {
                    // Found operator
                    if !current.trim().is_empty() {
                        parts.push(current.trim().to_string());
                    }
                    current = String::new();
                    i += op_bytes.len();
                } else {
                    current.push(bytes[i] as char);
                    i += 1;
                }
            } else {
                current.push(bytes[i] as char);
                i += 1;
            }
        }

        if !current.trim().is_empty() {
            parts.push(current.trim().to_string());
        }

        if parts.is_empty() {
            vec![input.to_string()]
        } else {
            parts
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_term() {
        let ast = LuceneParser::parse("auth").unwrap();
        match ast {
            QueryNode::Term(t) => assert_eq!(t.value, "auth"),
            _ => panic!("Expected Term"),
        }
    }

    #[test]
    fn test_parse_field_term() {
        let ast = LuceneParser::parse("type:error").unwrap();
        match ast {
            QueryNode::Term(t) => {
                assert_eq!(t.field, Some("type".to_string()));
                assert_eq!(t.value, "error");
            }
            _ => panic!("Expected Term"),
        }
    }

    #[test]
    fn test_empty_query() {
        let result = LuceneParser::parse("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_and_operator() {
        let ast = LuceneParser::parse("error AND warning").unwrap();
        match ast {
            QueryNode::And(children) => {
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Expected And node, got {:?}", ast),
        }
    }

    #[test]
    fn test_parse_or_operator() {
        let ast = LuceneParser::parse("error OR warning").unwrap();
        match ast {
            QueryNode::Or(children) => {
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Expected Or node, got {:?}", ast),
        }
    }

    #[test]
    fn test_parse_not_operator() {
        let ast = LuceneParser::parse("NOT error").unwrap();
        match ast {
            QueryNode::Not(child) => match child.as_ref() {
                QueryNode::Term(t) => assert_eq!(t.value, "error"),
                _ => panic!("Expected Term inside Not"),
            },
            _ => panic!("Expected Not node, got {:?}", ast),
        }
    }

    #[test]
    fn test_parse_parentheses() {
        let ast = LuceneParser::parse("(error OR warning) AND critical").unwrap();
        match ast {
            QueryNode::And(children) => {
                assert_eq!(children.len(), 2);
                match &children[0] {
                    QueryNode::Or(or_children) => assert_eq!(or_children.len(), 2),
                    _ => panic!("Expected Or as first child"),
                }
            }
            _ => panic!("Expected And node, got {:?}", ast),
        }
    }

    #[test]
    fn test_parse_boost() {
        let ast = LuceneParser::parse("error^2.0").unwrap();
        match ast {
            QueryNode::Boost { query: _, boost } => {
                assert_eq!(boost, 2.0);
            }
            _ => panic!("Expected Boost node, got {:?}", ast),
        }
    }

    #[test]
    fn test_parse_complex_query() {
        let ast =
            LuceneParser::parse("(type:error OR type:warning) AND NOT status:resolved").unwrap();
        match ast {
            QueryNode::And(children) => {
                assert_eq!(children.len(), 2);
            }
            _ => panic!("Expected And node, got {:?}", ast),
        }
    }
}
