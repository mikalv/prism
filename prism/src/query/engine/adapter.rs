//! Query AST to Tantivy Query conversion adapter

use crate::query::ast::{QueryNode, TermQuery};
use crate::query::{QueryError, Result};
use std::collections::HashMap;
use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, Occur, PhraseQuery, Query, TermQuery as TantivyTermQuery,
};
use tantivy::schema::{Field, Schema};
use tantivy::Term;

/// Stateless adapter that converts QueryNode AST to Tantivy Query objects
pub struct QueryAdapter;

impl QueryAdapter {
    /// Convert a QueryNode AST to a Tantivy Query object
    pub fn convert(
        node: &QueryNode,
        schema: &Schema,
        field_map: &HashMap<String, Field>,
        default_fields: &[Field],
    ) -> Result<Box<dyn Query>> {
        match node {
            QueryNode::Term(term) => Self::convert_term(term, schema, field_map, default_fields),
            QueryNode::And(children) => {
                Self::convert_and(children, schema, field_map, default_fields)
            }
            QueryNode::Or(children) => {
                Self::convert_or(children, schema, field_map, default_fields)
            }
            QueryNode::Not(child) => Self::convert_not(child, schema, field_map, default_fields),
            QueryNode::Boost { query, boost } => {
                Self::convert_boost(query, *boost, schema, field_map, default_fields)
            }
            QueryNode::Phrase(phrase) => {
                Self::convert_phrase(phrase, schema, field_map, default_fields)
            }
            QueryNode::Wildcard(_) => Err(QueryError::ExecutionError(
                "Wildcard queries not yet implemented (Phase 2)".to_string(),
            )),
            QueryNode::Range(_) => Err(QueryError::ExecutionError(
                "Range queries not yet implemented (Phase 2)".to_string(),
            )),
        }
    }

    // Note: schema parameter kept for API consistency; will be used for field type validation in Phase 2
    fn convert_term(
        term: &TermQuery,
        _schema: &Schema,
        field_map: &HashMap<String, Field>,
        default_fields: &[Field],
    ) -> Result<Box<dyn Query>> {
        if let Some(field_name) = &term.field {
            // Field-specific query
            let field = field_map
                .get(field_name)
                .ok_or_else(|| QueryError::InvalidField(field_name.clone()))?;
            let tantivy_term = Term::from_field_text(*field, &term.value);
            Ok(Box::new(TantivyTermQuery::new(
                tantivy_term,
                tantivy::schema::IndexRecordOption::Basic,
            )))
        } else {
            // No field specified - search across default fields with OR
            if default_fields.is_empty() {
                return Err(QueryError::SchemaError(
                    "No default fields configured".to_string(),
                ));
            }

            let subqueries: Vec<(Occur, Box<dyn Query>)> = default_fields
                .iter()
                .map(|field| {
                    let tantivy_term = Term::from_field_text(*field, &term.value);
                    let query: Box<dyn Query> = Box::new(TantivyTermQuery::new(
                        tantivy_term,
                        tantivy::schema::IndexRecordOption::Basic,
                    ));
                    (Occur::Should, query)
                })
                .collect();

            Ok(Box::new(BooleanQuery::new(subqueries)))
        }
    }

    fn convert_and(
        children: &[QueryNode],
        schema: &Schema,
        field_map: &HashMap<String, Field>,
        default_fields: &[Field],
    ) -> Result<Box<dyn Query>> {
        let subqueries: Result<Vec<(Occur, Box<dyn Query>)>> = children
            .iter()
            .map(|child| {
                let query = Self::convert(child, schema, field_map, default_fields)?;
                Ok((Occur::Must, query))
            })
            .collect();

        Ok(Box::new(BooleanQuery::new(subqueries?)))
    }

    fn convert_or(
        children: &[QueryNode],
        schema: &Schema,
        field_map: &HashMap<String, Field>,
        default_fields: &[Field],
    ) -> Result<Box<dyn Query>> {
        let subqueries: Result<Vec<(Occur, Box<dyn Query>)>> = children
            .iter()
            .map(|child| {
                let query = Self::convert(child, schema, field_map, default_fields)?;
                Ok((Occur::Should, query))
            })
            .collect();

        Ok(Box::new(BooleanQuery::new(subqueries?)))
    }

    fn convert_not(
        child: &QueryNode,
        schema: &Schema,
        field_map: &HashMap<String, Field>,
        default_fields: &[Field],
    ) -> Result<Box<dyn Query>> {
        let inner = Self::convert(child, schema, field_map, default_fields)?;
        // Tantivy requires at least one positive clause with MustNot
        // Use a MatchAll query paired with MustNot
        let subqueries = vec![
            (Occur::Must, Box::new(AllQuery) as Box<dyn Query>),
            (Occur::MustNot, inner),
        ];
        Ok(Box::new(BooleanQuery::new(subqueries)))
    }

    fn convert_boost(
        query: &QueryNode,
        boost: f32,
        schema: &Schema,
        field_map: &HashMap<String, Field>,
        default_fields: &[Field],
    ) -> Result<Box<dyn Query>> {
        let inner = Self::convert(query, schema, field_map, default_fields)?;
        Ok(Box::new(BoostQuery::new(inner, boost)))
    }

    // Note: schema parameter kept for API consistency; will be used for field type validation in Phase 2
    fn convert_phrase(
        phrase: &crate::query::ast::PhraseQuery,
        _schema: &Schema,
        field_map: &HashMap<String, Field>,
        default_fields: &[Field],
    ) -> Result<Box<dyn Query>> {
        let fields = if let Some(field_name) = &phrase.field {
            let field = field_map
                .get(field_name)
                .ok_or_else(|| QueryError::InvalidField(field_name.clone()))?;
            vec![*field]
        } else {
            if default_fields.is_empty() {
                return Err(QueryError::SchemaError(
                    "No default fields for phrase".to_string(),
                ));
            }
            default_fields.to_vec()
        };

        // For multiple fields, create OR of phrase queries
        if fields.len() == 1 {
            let field = fields[0];
            let terms: Vec<Term> = phrase
                .terms
                .iter()
                .map(|t| Term::from_field_text(field, t))
                .collect();
            Ok(Box::new(PhraseQuery::new(terms)))
        } else {
            let subqueries: Vec<(Occur, Box<dyn Query>)> = fields
                .iter()
                .map(|field| {
                    let terms: Vec<Term> = phrase
                        .terms
                        .iter()
                        .map(|t| Term::from_field_text(*field, t))
                        .collect();
                    let pq: Box<dyn Query> = Box::new(PhraseQuery::new(terms));
                    (Occur::Should, pq)
                })
                .collect();
            Ok(Box::new(BooleanQuery::new(subqueries)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::schema::{Schema, STORED, TEXT};

    fn test_schema() -> (Schema, HashMap<String, Field>) {
        let mut schema_builder = Schema::builder();
        let title = schema_builder.add_text_field("title", TEXT | STORED);
        let content = schema_builder.add_text_field("content", TEXT);
        let schema = schema_builder.build();

        let mut field_map = HashMap::new();
        field_map.insert("title".to_string(), title);
        field_map.insert("content".to_string(), content);

        (schema, field_map)
    }

    #[test]
    fn test_convert_simple_term() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![*field_map.get("title").unwrap()];

        let node = QueryNode::term("error");
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        assert!(result.is_ok(), "Should convert simple term");
    }

    #[test]
    fn test_convert_field_term() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![];

        let node = QueryNode::field_term("title", "error");
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        assert!(result.is_ok(), "Should convert field term");
    }

    #[test]
    fn test_convert_invalid_field() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![];

        let node = QueryNode::field_term("nonexistent", "error");
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        assert!(result.is_err(), "Should error on invalid field");
        match result {
            Err(QueryError::InvalidField(f)) => assert_eq!(f, "nonexistent"),
            _ => panic!("Expected InvalidField error"),
        }
    }

    #[test]
    fn test_convert_and_query() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![*field_map.get("title").unwrap()];

        let node = QueryNode::term("error").and(QueryNode::term("critical"));
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        assert!(result.is_ok(), "Should convert AND query");
    }

    #[test]
    fn test_convert_or_query() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![*field_map.get("title").unwrap()];

        let node = QueryNode::term("error").or(QueryNode::term("warning"));
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        assert!(result.is_ok(), "Should convert OR query");
    }

    #[test]
    fn test_convert_not_query() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![*field_map.get("title").unwrap()];

        let node = QueryNode::term("error").negate();
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        assert!(result.is_ok(), "Should convert NOT query");
    }

    #[test]
    fn test_convert_boost_query() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![*field_map.get("title").unwrap()];

        let node = QueryNode::term("important").boost(2.0);
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        assert!(result.is_ok(), "Should convert boost query");
    }

    #[test]
    fn test_convert_phrase_query() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![*field_map.get("title").unwrap()];

        let node = QueryNode::phrase(vec!["hello".to_string(), "world".to_string()]);
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        assert!(result.is_ok(), "Should convert phrase query");
    }

    #[test]
    fn test_convert_wildcard_returns_not_implemented() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![];

        let node = QueryNode::Wildcard(crate::query::ast::WildcardQuery {
            field: Some("title".to_string()),
            pattern: "err*".to_string(),
        });
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        // For now, wildcards return error (Phase 2 feature)
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_range_returns_not_implemented() {
        let (schema, field_map) = test_schema();
        let default_fields = vec![];

        let node = QueryNode::Range(crate::query::ast::RangeQuery {
            field: "timestamp".to_string(),
            lower: Some("2024-01-01".to_string()),
            upper: None,
            inclusive: true,
        });
        let result = QueryAdapter::convert(&node, &schema, &field_map, &default_fields);

        // For now, ranges return error (Phase 2 feature)
        assert!(result.is_err());
    }
}
