pub mod adapter;
pub mod boosting;
pub mod field_extraction;
pub mod telemetry;

pub use adapter::QueryAdapter;
pub use field_extraction::{
    convert_doc_to_map, extract_context_fields, extract_field_value, extract_timestamp,
};

/// Extract context fields (project_id, session_id, file_path) from document field map
fn extract_context_from_fields(fields: &HashMap<String, serde_json::Value>) -> HashMap<String, String> {
    let mut context = HashMap::new();
    for key in &["project_id", "session_id", "file_path"] {
        if let Some(value) = fields.get(*key) {
            if let Some(s) = value.as_str() {
                context.insert(key.to_string(), s.to_string());
            }
        }
    }
    context
}
pub use telemetry::{QueryMetrics, QueryTelemetry};

use super::aggregations::{AggregationRequest, AggregationResult};
use super::ast::QueryNode;
use super::{QueryError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tantivy::collector::TopDocs;
use tantivy::schema::{Field, Schema};
use tantivy::{Index, IndexReader};

/// Search result item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, serde_json::Value>,
}

/// Query execution options
#[derive(Debug, Clone, Default)]
pub struct QueryOptions {
    pub limit: usize,
    pub offset: usize,
    pub facet_requests: Vec<AggregationRequest>,
    pub boosting_config: Option<BoostingConfig>,
    pub search_context: SearchContext,
}

/// Boosting configuration
#[derive(Debug, Clone, Default)]
pub struct BoostingConfig {
    pub recency_enabled: bool,
    pub recency_field: String,
    pub recency_decay_days: f64,
    pub context_boost: f32,
    pub field_weights: HashMap<String, f32>,
}

/// Search context for context-aware boosting
#[derive(Debug, Clone, Default)]
pub struct SearchContext {
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub file_path: Option<String>,
}

impl SearchContext {
    pub fn to_hashmap(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Some(ref pid) = self.project_id {
            map.insert("project_id".to_string(), pid.clone());
        }
        if let Some(ref sid) = self.session_id {
            map.insert("session_id".to_string(), sid.clone());
        }
        if let Some(ref fp) = self.file_path {
            map.insert("file_path".to_string(), fp.clone());
        }
        map
    }
}

/// Query execution results
#[derive(Debug, Clone)]
pub struct QueryResults {
    pub results: Vec<SearchResult>,
    pub total_matches: usize,
    pub facets: Vec<AggregationResult>,
}

/// Orchestrates query execution pipeline
pub struct QueryExecutor {
    #[allow(dead_code)]
    index: Index,
    reader: IndexReader,
    schema: Schema,
    field_map: HashMap<String, Field>,
    default_fields: Vec<Field>,
}

impl QueryExecutor {
    /// Create a new QueryExecutor for an index
    pub fn new(
        index: Index,
        reader: IndexReader,
        schema: Schema,
        field_map: HashMap<String, Field>,
        default_fields: Vec<Field>,
    ) -> Self {
        Self {
            index,
            reader,
            schema,
            field_map,
            default_fields,
        }
    }

    /// Execute a query and return results with metrics
    pub fn execute(
        &self,
        ast: &QueryNode,
        options: QueryOptions,
    ) -> Result<(QueryResults, QueryMetrics)> {
        let mut telemetry = QueryTelemetry::new();

        // 1. Convert AST to Tantivy Query
        telemetry.mark_stage("ast_convert");
        let query =
            QueryAdapter::convert(ast, &self.schema, &self.field_map, &self.default_fields)?;

        // 2. Execute Tantivy search
        telemetry.mark_stage("tantivy_search");
        let searcher = self.reader.searcher();
        let limit = options.limit + options.offset;
        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| QueryError::TantivyError(e.to_string()))?;

        let total_matches = top_docs.len();

        // 3. Extract results (skip offset, take limit)
        let mut results = Vec::new();
        let id_field = self.field_map.get("id");

        for (score, doc_address) in top_docs
            .into_iter()
            .skip(options.offset)
            .take(options.limit)
        {
            let doc: tantivy::TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| QueryError::TantivyError(e.to_string()))?;

            let id = match id_field.and_then(|f| extract_field_value(&doc, *f)) {
                Some(id) => id,
                None => {
                    tracing::error!("Document missing 'id' field at address {:?}", doc_address);
                    continue;
                }
            };

            let fields = convert_doc_to_map(&doc, &self.schema, &self.field_map);

            results.push(SearchResult { id, score, fields });
        }

        // 4. Compute facets from result documents
        telemetry.mark_stage("facet_compute");
        let result_docs: Vec<_> = results.iter().map(|r| r.fields.clone()).collect();
        let facets = if !options.facet_requests.is_empty() {
            super::aggregations::facets::compute_facets(options.facet_requests.clone(), &result_docs)
                .unwrap_or_else(|e| {
                    tracing::warn!("Facet computation failed: {}", e);
                    Vec::new()
                })
        } else {
            Vec::new()
        };

        // 5. Apply boosting
        telemetry.mark_stage("boost_apply");
        if let Some(ref boost_config) = options.boosting_config {
            use super::boosting::{
                apply_boost, calculate_context_boost, calculate_recency_decay, DecayFunction,
            };
            use chrono::{DateTime, Duration, Utc};

            let now = Utc::now();
            let search_context = options.search_context.to_hashmap();

            for result in &mut results {
                let mut recency_mult = 1.0;
                let mut context_mult = 1.0;

                // Recency boost
                if boost_config.recency_enabled {
                    if let Some(ts_value) = result.fields.get(&boost_config.recency_field) {
                        if let Some(ts_str) = ts_value.as_str() {
                            if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
                                recency_mult = calculate_recency_decay(
                                    dt.with_timezone(&Utc),
                                    now,
                                    DecayFunction::Exponential,
                                    Duration::days(boost_config.recency_decay_days as i64),
                                    Duration::days(1),
                                    0.5,
                                );
                            }
                        }
                    }
                }

                // Context boost
                if boost_config.context_boost > 1.0 {
                    let doc_context = extract_context_from_fields(&result.fields);
                    context_mult = calculate_context_boost(
                        &doc_context,
                        &search_context,
                        boost_config.context_boost,
                    );
                }

                result.score = apply_boost(result.score, recency_mult, context_mult, 1.0);
            }

            // Re-sort by boosted score
            results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        }

        // 6. Build metrics
        let metrics = telemetry.finish(
            "", // collection name would come from caller
            &Self::query_type(ast),
            results.len(),
            total_matches,
            facets.len(),
        );

        Ok((
            QueryResults {
                results,
                total_matches,
                facets,
            },
            metrics,
        ))
    }

    fn query_type(ast: &QueryNode) -> String {
        match ast {
            QueryNode::Term(_) => "term".to_string(),
            QueryNode::Phrase(_) => "phrase".to_string(),
            QueryNode::And(_) => "and".to_string(),
            QueryNode::Or(_) => "or".to_string(),
            QueryNode::Not(_) => "not".to_string(),
            QueryNode::Boost { .. } => "boost".to_string(),
            QueryNode::Wildcard(_) => "wildcard".to_string(),
            QueryNode::Range(_) => "range".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::schema::{Schema, STORED, STRING, TEXT};
    use tantivy::{Index, TantivyDocument};

    fn setup_test_index() -> (
        Index,
        IndexReader,
        Schema,
        HashMap<String, Field>,
        Vec<Field>,
    ) {
        let mut schema_builder = Schema::builder();
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let title_field = schema_builder.add_text_field("title", TEXT | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT | STORED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema.clone());

        // Index some test documents
        let mut writer = index.writer(50_000_000).unwrap();

        let mut doc1 = TantivyDocument::new();
        doc1.add_text(id_field, "doc1");
        doc1.add_text(title_field, "Error handling in Rust");
        doc1.add_text(content_field, "This is about error handling");
        writer.add_document(doc1).unwrap();

        let mut doc2 = TantivyDocument::new();
        doc2.add_text(id_field, "doc2");
        doc2.add_text(title_field, "Warning messages");
        doc2.add_text(content_field, "This document has warnings");
        writer.add_document(doc2).unwrap();

        writer.commit().unwrap();

        let reader = index.reader().unwrap();

        let mut field_map = HashMap::new();
        field_map.insert("id".to_string(), id_field);
        field_map.insert("title".to_string(), title_field);
        field_map.insert("content".to_string(), content_field);

        let default_fields = vec![title_field, content_field];

        (index, reader, schema, field_map, default_fields)
    }

    #[test]
    fn test_executor_simple_term_query() {
        let (index, reader, schema, field_map, default_fields) = setup_test_index();
        let executor = QueryExecutor::new(index, reader, schema, field_map, default_fields);

        let ast = QueryNode::term("error");
        let options = QueryOptions {
            limit: 10,
            ..Default::default()
        };

        let (results, metrics) = executor.execute(&ast, options).unwrap();

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].id, "doc1");
        assert!(metrics.total_ms > 0.0);
    }

    #[test]
    fn test_executor_field_query() {
        let (index, reader, schema, field_map, default_fields) = setup_test_index();
        let executor = QueryExecutor::new(index, reader, schema, field_map, default_fields);

        let ast = QueryNode::field_term("title", "warning");
        let options = QueryOptions {
            limit: 10,
            ..Default::default()
        };

        let (results, _) = executor.execute(&ast, options).unwrap();

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].id, "doc2");
    }

    #[test]
    fn test_executor_and_query() {
        let (index, reader, schema, field_map, default_fields) = setup_test_index();
        let executor = QueryExecutor::new(index, reader, schema, field_map, default_fields);

        let ast = QueryNode::term("error").and(QueryNode::term("rust"));
        let options = QueryOptions {
            limit: 10,
            ..Default::default()
        };

        let (results, _) = executor.execute(&ast, options).unwrap();

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].id, "doc1");
    }

    #[test]
    fn test_executor_or_query() {
        let (index, reader, schema, field_map, default_fields) = setup_test_index();
        let executor = QueryExecutor::new(index, reader, schema, field_map, default_fields);

        let ast = QueryNode::term("error").or(QueryNode::term("warning"));
        let options = QueryOptions {
            limit: 10,
            ..Default::default()
        };

        let (results, _) = executor.execute(&ast, options).unwrap();

        assert_eq!(results.results.len(), 2);
    }

    #[test]
    fn test_executor_pagination() {
        let (index, reader, schema, field_map, default_fields) = setup_test_index();
        let executor = QueryExecutor::new(index, reader, schema, field_map, default_fields);

        let ast = QueryNode::term("error").or(QueryNode::term("warning"));

        // First page
        let options1 = QueryOptions {
            limit: 1,
            offset: 0,
            ..Default::default()
        };
        let (results1, _) = executor.execute(&ast, options1).unwrap();
        assert_eq!(results1.results.len(), 1);

        // Second page
        let options2 = QueryOptions {
            limit: 1,
            offset: 1,
            ..Default::default()
        };
        let (results2, _) = executor.execute(&ast, options2).unwrap();
        assert_eq!(results2.results.len(), 1);

        // Different results
        assert_ne!(results1.results[0].id, results2.results[0].id);
    }

    #[test]
    fn test_executor_computes_facets() {
        use super::super::aggregations::{AggregationRequest, AggregationType};

        let (index, reader, schema, field_map, default_fields) = setup_test_index();
        let executor = QueryExecutor::new(index, reader, schema, field_map, default_fields);

        let ast = QueryNode::term("error").or(QueryNode::term("warning"));
        let options = QueryOptions {
            limit: 10,
            facet_requests: vec![AggregationRequest {
                field: "title".to_string(),
                agg_type: AggregationType::Terms,
                size: 10,
                interval: None,
            }],
            ..Default::default()
        };

        let (results, _) = executor.execute(&ast, options).unwrap();

        assert!(!results.facets.is_empty(), "Facets should be computed");
    }

    #[test]
    fn test_executor_applies_boosting() {
        use tantivy::schema::DateOptions;
        use chrono::{Duration, Utc};

        // Setup index with timestamp field
        let mut schema_builder = Schema::builder();
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let title_field = schema_builder.add_text_field("title", TEXT | STORED);
        let timestamp_field = schema_builder.add_date_field("timestamp", DateOptions::default().set_stored());
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema.clone());
        let mut writer = index.writer(50_000_000).unwrap();

        let now = Utc::now();
        let old = now - Duration::days(60);

        // Old document
        let mut doc1 = TantivyDocument::new();
        doc1.add_text(id_field, "old_doc");
        doc1.add_text(title_field, "rust programming");
        doc1.add_date(timestamp_field, tantivy::DateTime::from_timestamp_secs(old.timestamp()));
        writer.add_document(doc1).unwrap();

        // Recent document
        let mut doc2 = TantivyDocument::new();
        doc2.add_text(id_field, "new_doc");
        doc2.add_text(title_field, "rust programming");
        doc2.add_date(timestamp_field, tantivy::DateTime::from_timestamp_secs(now.timestamp()));
        writer.add_document(doc2).unwrap();

        writer.commit().unwrap();
        let reader = index.reader().unwrap();

        let mut field_map = HashMap::new();
        field_map.insert("id".to_string(), id_field);
        field_map.insert("title".to_string(), title_field);
        field_map.insert("timestamp".to_string(), timestamp_field);

        let executor = QueryExecutor::new(
            index,
            reader,
            schema,
            field_map,
            vec![title_field],
        );

        let ast = QueryNode::term("rust");
        let options = QueryOptions {
            limit: 10,
            boosting_config: Some(BoostingConfig {
                recency_enabled: true,
                recency_field: "timestamp".to_string(),
                recency_decay_days: 30.0,
                context_boost: 1.0,
                field_weights: HashMap::new(),
            }),
            ..Default::default()
        };

        let (results, _) = executor.execute(&ast, options).unwrap();

        assert_eq!(results.results.len(), 2);
        // Recent document should be first after boosting
        assert_eq!(results.results[0].id, "new_doc", "Recent doc should rank higher");
    }
}
