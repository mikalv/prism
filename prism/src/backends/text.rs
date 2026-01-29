use crate::backends::{BackendStats, Document, Query, SearchBackend, SearchResult, SearchResults, SearchResultsWithAggs};
use crate::schema::{CollectionSchema, FieldType};
use crate::{Error, Result};
use crate::aggregations::{AggregationRequest, AggregationType, AggregationResult, AggregationValue, Bucket};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tantivy::{
     collector::TopDocs, query::QueryParser, schema::*,
     Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument,
     Term,
};
use tantivy::aggregation::agg_req::Aggregations;
use tantivy::aggregation::agg_result::AggregationResults;
use tantivy::aggregation::AggregationCollector;

pub struct TextBackend {
    base_path: PathBuf,
    collections: Arc<RwLock<HashMap<String, CollectionIndex>>>,
}

struct CollectionIndex {
    index: Index,
    schema: Schema,
    field_map: HashMap<String, Field>,
    reader: IndexReader,
    writer: Arc<parking_lot::Mutex<IndexWriter>>,
}

impl TextBackend {
    pub fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_path)?;

        Ok(Self {
            base_path,
            collections: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn initialize(&self, collection: &str, schema: &CollectionSchema) -> Result<()> {
        let text_config = schema
            .backends
            .text
            .as_ref()
            .ok_or_else(|| Error::Schema("No text backend config".to_string()))?;

        // Build Tantivy schema
        let mut schema_builder = Schema::builder();
        let mut field_map = HashMap::new();

        // Add ID field (always present)
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        field_map.insert("id".to_string(), id_field);

        // Add configured fields
        for field_def in &text_config.fields {
            let field = match field_def.field_type {
                FieldType::Text => {
                    let mut options = TextOptions::default();
                    if field_def.indexed {
                        options = options.set_indexing_options(
                            TextFieldIndexing::default()
                                .set_tokenizer("default")
                                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                        );
                    }
                    if field_def.stored {
                        options = options.set_stored();
                    }
                    schema_builder.add_text_field(&field_def.name, options)
                }
                FieldType::String => {
                    let mut opts = STRING;
                    if field_def.stored {
                        opts = opts | STORED;
                    }
                    schema_builder.add_text_field(&field_def.name, opts)
                }
                FieldType::I64 => {
                    let mut opts = NumericOptions::default().set_indexed();
                    if field_def.stored {
                        opts = opts.set_stored();
                    }
                    schema_builder.add_i64_field(&field_def.name, opts)
                }
                FieldType::U64 => {
                    let mut opts = NumericOptions::default().set_indexed();
                    if field_def.stored {
                        opts = opts.set_stored();
                    }
                    schema_builder.add_u64_field(&field_def.name, opts)
                }
                FieldType::F64 => {
                    let mut opts = NumericOptions::default().set_indexed();
                    if field_def.stored {
                        opts = opts.set_stored();
                    }
                    schema_builder.add_f64_field(&field_def.name, opts)
                }
                FieldType::Bool => {
                    let mut opts = NumericOptions::default().set_indexed();
                    if field_def.stored {
                        opts = opts.set_stored();
                    }
                    schema_builder.add_bool_field(&field_def.name, opts)
                }
                FieldType::Date => {
                    let mut opts = DateOptions::default().set_indexed();
                    if field_def.stored {
                        opts = opts.set_stored();
                    }
                    schema_builder.add_date_field(&field_def.name, opts)
                }
                FieldType::Bytes => schema_builder.add_bytes_field(&field_def.name, STORED),
            };

            field_map.insert(field_def.name.clone(), field);
        }

        // Create or open index
        let index_path = self.base_path.join(collection);
        std::fs::create_dir_all(&index_path)?;

        // Try to open existing index, otherwise create new one
        let (index, schema, field_map) = match Index::open_in_dir(&index_path) {
            Ok(existing_index) => {
                // Use the existing index's schema and rebuild field_map from it
                let existing_schema = existing_index.schema();
                let mut existing_field_map = HashMap::new();
                for (field, entry) in existing_schema.fields() {
                    existing_field_map.insert(entry.name().to_string(), field);
                }
                (existing_index, existing_schema, existing_field_map)
            }
            Err(_) => {
                let schema = schema_builder.build();
                let new_index = Index::create_in_dir(&index_path, schema.clone())?;
                (new_index, schema, field_map)
            }
        };

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let writer = Arc::new(parking_lot::Mutex::new(index.writer(50_000_000)?));

        let collection_index = CollectionIndex {
            index,
            schema,
            field_map,
            reader,
            writer,
        };

        self.collections
            .write()
            .unwrap()
            .insert(collection.to_string(), collection_index);

        Ok(())
    }

    #[cfg(feature = "storage-s3")]
    pub async fn initialize_with_storage(
        &self,
        collection: &str,
        schema: &CollectionSchema,
        storage: &crate::storage::StorageConfig,
    ) -> Result<()> {
        use crate::storage::{LocalConfig, ObjectStoreDirectory, S3Config, StorageConfig};

        match storage {
            StorageConfig::Local(_) => self.initialize(collection, schema).await,
            StorageConfig::S3(s3_config) => {
                let text_config = schema
                    .backends
                    .text
                    .as_ref()
                    .ok_or_else(|| Error::Schema("No text backend config".to_string()))?;

                let mut schema_builder = Schema::builder();
                let mut field_map = HashMap::new();

                let id_field = schema_builder.add_text_field("id", STRING | STORED);
                field_map.insert("id".to_string(), id_field);

                for field_def in &text_config.fields {
                    let field = match field_def.field_type {
                        FieldType::Text => {
                            let mut options = TextOptions::default();
                            if field_def.indexed {
                                options = options.set_indexing_options(
                                    TextFieldIndexing::default()
                                        .set_tokenizer("default")
                                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                                );
                            }
                            if field_def.stored {
                                options = options.set_stored();
                            }
                            schema_builder.add_text_field(&field_def.name, options)
                        }
                        FieldType::String => {
                            let mut opts = STRING;
                            if field_def.stored {
                                opts = opts | STORED;
                            }
                            schema_builder.add_text_field(&field_def.name, opts)
                        }
                        FieldType::I64 => {
                            let mut opts = NumericOptions::default().set_indexed();
                            if field_def.stored {
                                opts = opts.set_stored();
                            }
                            schema_builder.add_i64_field(&field_def.name, opts)
                        }
                        FieldType::U64 => {
                            let mut opts = NumericOptions::default().set_indexed();
                            if field_def.stored {
                                opts = opts.set_stored();
                            }
                            schema_builder.add_u64_field(&field_def.name, opts)
                        }
                        FieldType::F64 => {
                            let mut opts = NumericOptions::default().set_indexed();
                            if field_def.stored {
                                opts = opts.set_stored();
                            }
                            schema_builder.add_f64_field(&field_def.name, opts)
                        }
                        FieldType::Bool => {
                            let mut opts = NumericOptions::default().set_indexed();
                            if field_def.stored {
                                opts = opts.set_stored();
                            }
                            schema_builder.add_bool_field(&field_def.name, opts)
                        }
                        FieldType::Date => {
                            let mut opts = DateOptions::default().set_indexed();
                            if field_def.stored {
                                opts = opts.set_stored();
                            }
                            schema_builder.add_date_field(&field_def.name, opts)
                        }
                        FieldType::Bytes => schema_builder.add_bytes_field(&field_def.name, STORED),
                    };
                    field_map.insert(field_def.name.clone(), field);
                }

                let tantivy_schema = schema_builder.build();

                let directory =
                    ObjectStoreDirectory::from_s3_config(s3_config, collection, None, 0)
                        .await
                        .map_err(|e| Error::Io(e))?;

                let index = if directory.is_empty() {
                    Index::create(
                        directory,
                        tantivy_schema.clone(),
                        tantivy::IndexSettings::default(),
                    )?
                } else {
                    Index::open(directory)?
                };

                let existing_schema = index.schema();
                let mut existing_field_map = HashMap::new();
                for (field, entry) in existing_schema.fields() {
                    existing_field_map.insert(entry.name().to_string(), field);
                }

                let reader = index
                    .reader_builder()
                    .reload_policy(ReloadPolicy::OnCommitWithDelay)
                    .try_into()?;

                let writer = Arc::new(parking_lot::Mutex::new(index.writer(50_000_000)?));

                let collection_index = CollectionIndex {
                    index,
                    schema: existing_schema,
                    field_map: existing_field_map,
                    reader,
                    writer,
                };

                self.collections
                    .write()
                    .unwrap()
                    .insert(collection.to_string(), collection_index);

                Ok(())
            }
        }
    }
}

#[async_trait]
impl SearchBackend for TextBackend {
    async fn index(&self, collection: &str, docs: Vec<Document>) -> Result<()> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let mut writer = coll.writer.lock();

        for doc in docs {
            let mut tantivy_doc = TantivyDocument::new();

            // Add ID
            let id_field = coll.field_map.get("id").unwrap();
            tantivy_doc.add_text(*id_field, &doc.id);

            // Add other fields
            for (field_name, value) in doc.fields {
                if let Some(field) = coll.field_map.get(&field_name) {
                    let field_entry = coll.schema.get_field_entry(*field);
                    let field_type = field_entry.field_type();

                    let added = match (&value, field_type) {
                        // String/text values
                        (serde_json::Value::String(s), _) => {
                            tantivy_doc.add_text(*field, s);
                            true
                        }

                        // Numbers - match against schema type for proper conversion
                        (serde_json::Value::Number(n), tantivy::schema::FieldType::U64(_)) => {
                            // Try u64 first, then i64 (for positive values stored as i64)
                            if let Some(v) = n.as_u64() {
                                tantivy_doc.add_u64(*field, v);
                                true
                            } else if let Some(v) = n.as_i64() {
                                if v >= 0 {
                                    tantivy_doc.add_u64(*field, v as u64);
                                    true
                                } else {
                                    tracing::warn!(
                                        "Document '{}': negative i64 value {} cannot be stored in u64 field '{}'",
                                        doc.id, v, field_name
                                    );
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        (serde_json::Value::Number(n), tantivy::schema::FieldType::I64(_)) => {
                            if let Some(v) = n.as_i64() {
                                tantivy_doc.add_i64(*field, v);
                                true
                            } else if let Some(v) = n.as_u64() {
                                if v <= i64::MAX as u64 {
                                    tantivy_doc.add_i64(*field, v as i64);
                                    true
                                } else {
                                    tracing::warn!(
                                        "Document '{}': u64 value {} too large for i64 field '{}'",
                                        doc.id,
                                        v,
                                        field_name
                                    );
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        (serde_json::Value::Number(n), tantivy::schema::FieldType::F64(_)) => {
                            if let Some(v) = n.as_f64() {
                                tantivy_doc.add_f64(*field, v);
                                true
                            } else {
                                false
                            }
                        }
                        // Number to text field - convert to string
                        (serde_json::Value::Number(n), tantivy::schema::FieldType::Str(_)) => {
                            tantivy_doc.add_text(*field, &n.to_string());
                            true
                        }

                        (serde_json::Value::Bool(b), tantivy::schema::FieldType::Bool(_)) => {
                            tantivy_doc.add_bool(*field, *b);
                            true
                        }
                        (serde_json::Value::Bool(b), tantivy::schema::FieldType::Str(_)) => {
                            tantivy_doc.add_text(*field, if *b { "true" } else { "false" });
                            true
                        }

                        // Null values - skip silently
                        (serde_json::Value::Null, _) => true,

                        // Type mismatch - log warning
                        (val, ftype) => {
                            tracing::warn!(
                                "Document '{}': type mismatch for field '{}' - got {:?}, expected {:?}",
                                doc.id, field_name, val, ftype
                            );
                            false
                        }
                    };

                    if !added && !matches!(value, serde_json::Value::Null) {
                        tracing::debug!(
                            "Document '{}': failed to add field '{}' with value {:?}",
                            doc.id,
                            field_name,
                            value
                        );
                    }
                } else {
                    // Field not in schema - skip silently (could be extra fields in document)
                }
            }

            writer.add_document(tantivy_doc)?;
        }

        writer.commit()?;

        // Reload reader to see committed changes
        coll.reader.reload()?;

        Ok(())
    }

    async fn search(&self, collection: &str, query: Query) -> Result<SearchResults> {
        let start = std::time::Instant::now();

        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let searcher = coll.reader.searcher();

        // Build query - if no fields specified, use all TEXT fields (not numeric)
        let query_fields: Vec<Field> = if query.fields.is_empty() {
            // Default to only text-searchable fields (exclude id, numeric, and other non-text types)
            coll.field_map
                .iter()
                .filter(|(name, field)| {
                    // Skip id field
                    if *name == "id" {
                        return false;
                    }
                    // Only include fields that support text search (Str type with indexing)
                    let field_entry = coll.schema.get_field_entry(**field);
                    field_entry.field_type().is_indexed()
                        && matches!(field_entry.field_type(), tantivy::schema::FieldType::Str(_))
                })
                .map(|(_, field)| *field)
                .collect()
        } else {
            query
                .fields
                .iter()
                .filter_map(|f| coll.field_map.get(f).copied())
                .collect()
        };

        let query_parser = QueryParser::for_index(&coll.index, query_fields);
        let parsed_query = query_parser
            .parse_query(&query.query_string)
            .map_err(|e| Error::InvalidQuery(e.to_string()))?;

        // Execute search
        let top_docs = searcher.search(
            &parsed_query,
            &TopDocs::with_limit(query.limit + query.offset),
        )?;

        let mut results = Vec::new();
        let id_field = coll.field_map.get("id").unwrap();

        for (_score, doc_address) in top_docs.into_iter().skip(query.offset) {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;

            let id = retrieved_doc
                .get_first(*id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let mut fields = HashMap::new();
            for (field_name, field) in &coll.field_map {
                if let Some(value) = retrieved_doc.get_first(*field) {
                    let json_value = match value {
                        OwnedValue::Str(s) => serde_json::Value::String(s.clone()),
                        OwnedValue::U64(n) => serde_json::Value::Number((*n).into()),
                        OwnedValue::I64(n) => serde_json::Value::Number((*n).into()),
                        OwnedValue::F64(n) => {
                            serde_json::Value::Number(serde_json::Number::from_f64(*n).unwrap())
                        }
                        OwnedValue::Bool(b) => serde_json::Value::Bool(*b),
                        _ => continue,
                    };
                    fields.insert(field_name.clone(), json_value);
                }
            }

            results.push(SearchResult {
                id,
                score: _score,
                fields,
            });
        }

        let total = results.len();
        let latency_ms = start.elapsed().as_millis() as u64;

        Ok(SearchResults {
            results,
            total,
            latency_ms,
        })
    }

    async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>> {
        // Simplified: use search to find by ID
        let query = Query {
            query_string: format!("id:\"{}\"", id),
            fields: vec!["id".to_string()],
            limit: 1,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
        };

        let results = self.search(collection, query).await?;

        Ok(results.results.into_iter().next().map(|r| Document {
            id: r.id,
            fields: r.fields,
        }))
    }

    async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<()> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let mut writer = coll.writer.lock();
        let id_field = coll.field_map.get("id").unwrap();

        for id in ids {
            writer.delete_term(Term::from_field_text(*id_field, &id));
        }

        writer.commit()?;

        Ok(())
    }

    async fn stats(&self, collection: &str) -> Result<BackendStats> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let searcher = coll.reader.searcher();
        let document_count = searcher.num_docs() as usize;

        // Simplified size calculation
        let size_bytes = document_count * 1024; // Rough estimate

        Ok(BackendStats {
            document_count,
            size_bytes,
        })
    }

    async fn search_with_aggs(
        &self,
        collection: &str,
        query: &Query,
        aggregations: Vec<AggregationRequest>,
    ) -> Result<SearchResultsWithAggs> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let searcher = coll.reader.searcher();

        let query_fields: Vec<Field> = if query.fields.is_empty() {
            coll.field_map
                .iter()
                .filter(|(name, field)| {
                    if *name == "id" {
                        return false;
                    }
                    let field_entry = coll.schema.get_field_entry(**field);
                    field_entry.field_type().is_indexed()
                        && matches!(field_entry.field_type(), tantivy::schema::FieldType::Str(_))
                })
                .map(|(_, field)| *field)
                .collect()
        } else {
            query
                .fields
                .iter()
                .filter_map(|f| coll.field_map.get(f).copied())
                .collect()
        };

        let query_parser = QueryParser::for_index(&coll.index, query_fields);
        let parsed_query = query_parser
            .parse_query(&query.query_string)
            .map_err(|e| Error::InvalidQuery(e.to_string()))?;

        // Build Tantivy-native aggregation request from Prism's aggregation types
        let tantivy_agg_req = build_tantivy_aggregations(&aggregations)?;

        // Use Tantivy's native AggregationCollector
        let collector = AggregationCollector::from_aggs(tantivy_agg_req, Default::default());

        // Combine TopDocs with AggregationCollector
        let top_docs = TopDocs::with_limit(query.limit as usize);
        let (top_doc_results, agg_results) = searcher.search(&parsed_query, &(top_docs, collector))?;

        // Convert Tantivy aggregation results to Prism format
        let agg_results = convert_tantivy_agg_results(agg_results, &aggregations)?;

        // Build search results
        let mut results = Vec::new();
        let id_field = coll.field_map.get("id").unwrap();

        for (_score, doc_address) in top_doc_results {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;

            let id = retrieved_doc
                .get_first(*id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let fields = HashMap::new();
            // TODO: Implement proper value to JSON conversion for Tantivy 0.22

            results.push(SearchResult {
                id,
                score: _score,
                fields,
            });
        }

        let total = results.len() as u64;
        Ok(SearchResultsWithAggs {
            results,
            total,
            aggregations: agg_results,
        })
    }
}

/// Build Tantivy-native aggregation request from Prism's aggregation types
fn build_tantivy_aggregations(aggregations: &[AggregationRequest]) -> Result<Aggregations> {
    use serde_json::{json, Map, Value};

    let mut agg_map = Map::new();

    for agg in aggregations {
        let agg_def = match &agg.agg_type {
            AggregationType::Count => {
                json!({ "value_count": { "field": "_id" } })
            }
            AggregationType::Min { field } => {
                json!({ "min": { "field": field } })
            }
            AggregationType::Max { field } => {
                json!({ "max": { "field": field } })
            }
            AggregationType::Sum { field } => {
                json!({ "sum": { "field": field } })
            }
            AggregationType::Avg { field } => {
                json!({ "avg": { "field": field } })
            }
            AggregationType::Stats { field } => {
                json!({ "stats": { "field": field } })
            }
            AggregationType::Terms { field, size } => {
                json!({ "terms": { "field": field, "size": size.unwrap_or(10) } })
            }
            AggregationType::Histogram { field, interval } => {
                json!({ "histogram": { "field": field, "interval": interval } })
            }
            AggregationType::DateHistogram { field, calendar_interval } => {
                json!({ "date_histogram": { "field": field, "calendar_interval": calendar_interval } })
            }
        };

        agg_map.insert(agg.name.clone(), agg_def);
    }

    let agg_json = Value::Object(agg_map);
    serde_json::from_value(agg_json)
        .map_err(|e| Error::InvalidQuery(format!("Failed to build aggregation request: {}", e)))
}

/// Convert Tantivy aggregation results to Prism's format
fn convert_tantivy_agg_results(
    tantivy_results: AggregationResults,
    requests: &[AggregationRequest],
) -> Result<HashMap<String, AggregationResult>> {
    let mut results = HashMap::new();

    for req in requests {
        let result = match tantivy_results.0.get(&req.name) {
            Some(agg_result) => convert_single_agg_result(&req.name, &req.agg_type, agg_result)?,
            None => {
                // Aggregation not found in results, return default
                AggregationResult {
                    name: req.name.clone(),
                    value: AggregationValue::Single(0.0),
                }
            }
        };
        results.insert(req.name.clone(), result);
    }

    Ok(results)
}

fn convert_single_agg_result(
    name: &str,
    agg_type: &AggregationType,
    result: &tantivy::aggregation::agg_result::AggregationResult,
) -> Result<AggregationResult> {
    use tantivy::aggregation::agg_result::AggregationResult as TantivyAggResult;

    let value = match result {
        TantivyAggResult::BucketResult(bucket_result) => {
            // Handle bucket aggregations (terms, histogram, etc.)
            use tantivy::aggregation::agg_result::BucketEntries;
            match bucket_result {
                tantivy::aggregation::agg_result::BucketResult::Terms { buckets, .. } => {
                    let prism_buckets: Vec<Bucket> = buckets
                        .iter()
                        .map(|b| Bucket {
                            key: format!("{}", b.key),
                            doc_count: b.doc_count,
                            sub_aggs: None,
                        })
                        .collect();
                    AggregationValue::Buckets(prism_buckets)
                }
                tantivy::aggregation::agg_result::BucketResult::Histogram { buckets } => {
                    let entries = match buckets {
                        BucketEntries::Vec(v) => v.clone(),
                        BucketEntries::HashMap(m) => m.values().cloned().collect(),
                    };
                    let prism_buckets: Vec<Bucket> = entries
                        .iter()
                        .map(|b| Bucket {
                            key: b.key.to_string(),
                            doc_count: b.doc_count,
                            sub_aggs: None,
                        })
                        .collect();
                    AggregationValue::Buckets(prism_buckets)
                }
                tantivy::aggregation::agg_result::BucketResult::Range { buckets } => {
                    let entries = match buckets {
                        BucketEntries::Vec(v) => v.clone(),
                        BucketEntries::HashMap(m) => m.values().cloned().collect(),
                    };
                    let prism_buckets: Vec<Bucket> = entries
                        .iter()
                        .map(|b| Bucket {
                            key: b.key.to_string(),
                            doc_count: b.doc_count,
                            sub_aggs: None,
                        })
                        .collect();
                    AggregationValue::Buckets(prism_buckets)
                }
            }
        }
        TantivyAggResult::MetricResult(metric_result) => {
            use tantivy::aggregation::agg_result::MetricResult;
            match metric_result {
                MetricResult::Average(avg) => {
                    AggregationValue::Single(avg.value.unwrap_or(0.0))
                }
                MetricResult::Sum(sum) => {
                    AggregationValue::Single(sum.value.unwrap_or(0.0))
                }
                MetricResult::Min(min) => {
                    AggregationValue::Single(min.value.unwrap_or(0.0))
                }
                MetricResult::Max(max) => {
                    AggregationValue::Single(max.value.unwrap_or(0.0))
                }
                MetricResult::Count(count) => {
                    AggregationValue::Single(count.value.unwrap_or(0.0))
                }
                MetricResult::Stats(stats) => {
                    AggregationValue::Stats(crate::aggregations::types::StatsResult {
                        count: stats.count,
                        min: stats.min,
                        max: stats.max,
                        sum: Some(stats.sum),
                        avg: stats.avg,
                    })
                }
                _ => AggregationValue::Single(0.0),
            }
        }
    };

    Ok(AggregationResult {
        name: name.to_string(),
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_text_backend_index_and_search() -> Result<()> {
        let temp = TempDir::new()?;
        let backend = TextBackend::new(temp.path())?;

        // Create test schema
        let schema = CollectionSchema {
            collection: "test".to_string(),
            description: None,
            backends: crate::schema::Backends {
                text: Some(crate::schema::TextBackendConfig {
                    fields: vec![crate::schema::TextField {
                        name: "title".to_string(),
                        field_type: FieldType::Text,
                        stored: true,
                        indexed: true,
                    }],
                }),
                vector: None,
                graph: None,
            },
            indexing: Default::default(),
            quota: Default::default(),
            embedding_generation: None,
            facets: None,
            boosting: None,
            storage: Default::default(),
        };

        backend.initialize("test", &schema).await?;

        // Index document
        let doc = Document {
            id: "doc1".to_string(),
            fields: HashMap::from([("title".to_string(), json!("Hello World"))]),
        };

        backend.index("test", vec![doc]).await?;

        // Search
        let query = Query {
            query_string: "hello".to_string(),
            fields: vec!["title".to_string()],
            limit: 10,
            offset: 0,
            merge_strategy: None,
            text_weight: None,
            vector_weight: None,
        };

        let results = backend.search("test", query).await?;
        assert_eq!(results.total, 1);
        assert_eq!(results.results[0].id, "doc1");

        Ok(())
    }
}
