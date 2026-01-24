use serde::{Deserialize, Serialize};

/// Query abstract syntax tree node
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueryNode {
    /// Single term query: "auth" or "field:value"
    Term(TermQuery),

    /// Phrase query: "field:\"quoted phrase\""
    Phrase(PhraseQuery),

    /// Wildcard query: "auth*"
    Wildcard(WildcardQuery),

    /// Range query: "timestamp:[2024-01-01 TO *]"
    Range(RangeQuery),

    /// Boolean AND: a AND b
    And(Vec<QueryNode>),

    /// Boolean OR: a OR b
    Or(Vec<QueryNode>),

    /// Boolean NOT: NOT a or -a
    Not(Box<QueryNode>),

    /// Boosted query: term^2.0
    Boost { query: Box<QueryNode>, boost: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TermQuery {
    /// Field name (None = search all fields)
    pub field: Option<String>,
    /// Term value
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhraseQuery {
    pub field: Option<String>,
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WildcardQuery {
    pub field: Option<String>,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeQuery {
    pub field: String,
    pub lower: Option<String>,
    pub upper: Option<String>,
    pub inclusive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BooleanOp {
    And,
    Or,
    Not,
}

impl QueryNode {
    /// Create a simple term query
    pub fn term(value: impl Into<String>) -> Self {
        QueryNode::Term(TermQuery {
            field: None,
            value: value.into(),
        })
    }

    /// Create a field:value term query
    pub fn field_term(field: impl Into<String>, value: impl Into<String>) -> Self {
        QueryNode::Term(TermQuery {
            field: Some(field.into()),
            value: value.into(),
        })
    }

    /// Create a phrase query
    pub fn phrase(terms: Vec<String>) -> Self {
        QueryNode::Phrase(PhraseQuery { field: None, terms })
    }

    /// Combine with AND
    pub fn and(self, other: QueryNode) -> Self {
        match self {
            QueryNode::And(mut nodes) => {
                nodes.push(other);
                QueryNode::And(nodes)
            }
            _ => QueryNode::And(vec![self, other]),
        }
    }

    /// Combine with OR
    pub fn or(self, other: QueryNode) -> Self {
        match self {
            QueryNode::Or(mut nodes) => {
                nodes.push(other);
                QueryNode::Or(nodes)
            }
            _ => QueryNode::Or(vec![self, other]),
        }
    }

    /// Negate
    pub fn negate(self) -> Self {
        QueryNode::Not(Box::new(self))
    }

    /// Apply boost
    pub fn boost(self, boost: f32) -> Self {
        QueryNode::Boost {
            query: Box::new(self),
            boost,
        }
    }

    /// Get the query type as a string
    pub fn query_type(&self) -> &'static str {
        match self {
            QueryNode::Term(_) => "term",
            QueryNode::Phrase(_) => "phrase",
            QueryNode::And(_) => "and",
            QueryNode::Or(_) => "or",
            QueryNode::Not(_) => "not",
            QueryNode::Boost { .. } => "boost",
            QueryNode::Wildcard(_) => "wildcard",
            QueryNode::Range(_) => "range",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_term_query_builder() {
        let q = QueryNode::term("auth");
        assert_eq!(
            q,
            QueryNode::Term(TermQuery {
                field: None,
                value: "auth".to_string()
            })
        );
    }

    #[test]
    fn test_field_term_query() {
        let q = QueryNode::field_term("type", "error");
        match q {
            QueryNode::Term(t) => {
                assert_eq!(t.field, Some("type".to_string()));
                assert_eq!(t.value, "error");
            }
            _ => panic!("Expected Term"),
        }
    }

    #[test]
    fn test_and_combinator() {
        let q = QueryNode::term("auth").and(QueryNode::term("bug"));
        match q {
            QueryNode::And(nodes) => {
                assert_eq!(nodes.len(), 2);
            }
            _ => panic!("Expected And"),
        }
    }

    #[test]
    fn test_boost() {
        let q = QueryNode::term("important").boost(2.0);
        match q {
            QueryNode::Boost { query: _, boost } => {
                assert_eq!(boost, 2.0);
            }
            _ => panic!("Expected Boost"),
        }
    }
}
