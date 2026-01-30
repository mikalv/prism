//! Text search backend with unified SegmentStorage integration
//!
//! All storage (local, S3, cached) goes through the SegmentStorage trait via TantivyStorageAdapter.

use crate::backends::{BackendStats, Document, Query, SearchBackend, SearchResult, SearchResults, SearchResultsWithAggs};
use crate::schema::{CollectionSchema, FieldType};
use crate::{Error, Result};
use crate::aggregations::{AggregationRequest, AggregationResult, AggregationType, AggregationValue, Bucket};
use crate::aggregations::types::StatsResult;
use async_trait::async_trait;
use prism_storage::{LocalStorage, SegmentStorage, TantivyStorageAdapter};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tantivy::{
     collector::TopDocs, query::QueryParser, schema::*,
     Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument,
     Term, DateTime, DocSet, TERMINATED,
};
use tantivy::aggregation::agg_req::Aggregations;
use tantivy::aggregation::agg_result::AggregationResults;
use tantivy::aggregation::AggregationCollector;

pub struct TextBackend {
    /// Base path for local buffer directory (used for Tantivy temp files)
    base_path: PathBuf,
    /// Unified storage backend (local, S3, cached, etc.)
    storage: Arc<dyn SegmentStorage>,
    /// Collection indexes
    collections: Arc<RwLock<HashMap<String, CollectionIndex>>>,
}

struct CollectionIndex {
    index: Index,
    schema: Schema,
    field_map: HashMap<String, Field>,
    reader: IndexReader,
    writer: Arc<parking_lot::Mutex<IndexWriter>>,
    /// Whether _indexed_at system field is enabled
    indexed_at_enabled: bool,
    /// Whether _boost system field is enabled
    boost_enabled: bool,
}

impl TextBackend {
    /// Create a new TextBackend with local filesystem storage.
    ///
    /// Uses LocalStorage from prism-storage for persistence.
    pub fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_path)?;
        let storage = Arc::new(LocalStorage::new(&base_path));
        Self::with_segment_storage(base_path, storage)
    }

    /// Create a backend with unified SegmentStorage (local, S3, cached, etc.).
    ///
    /// This is the primary constructor - all storage goes through SegmentStorage.
    ///
    /// # Arguments
    ///
    /// * `buffer_path` - Local directory for Tantivy's temporary buffer files
    /// * `storage` - The SegmentStorage implementation to use
    pub fn with_segment_storage(
        buffer_path: impl AsRef<Path>,
        storage: Arc<dyn SegmentStorage>,
    ) -> Result<Self> {
        let base_path = buffer_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_path)?;

        Ok(Self {
            base_path,
            storage,
            collections: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Initialize a collection from schema.
    ///
    /// Creates or opens a Tantivy index using the unified SegmentStorage.
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

        // Add system fields based on configuration
        let system_fields = &schema.system_fields;

        // _indexed_at: timestamp when document was indexed (for recency scoring)
        if system_fields.indexed_at {
            let indexed_at_field = schema_builder.add_date_field(
                "_indexed_at",
                DateOptions::default().set_indexed().set_stored().set_fast(),
            );
            field_map.insert("_indexed_at".to_string(), indexed_at_field);
        }

        // _boost: per-document boost multiplier (for popularity signals)
        if system_fields.document_boost {
            let boost_field = schema_builder.add_f64_field(
                "_boost",
                NumericOptions::default().set_indexed().set_stored().set_fast(),
            );
            field_map.insert("_boost".to_string(), boost_field);
        }

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

        let tantivy_schema = schema_builder.build();

        // Create buffer directory for this collection
        let buffer_dir = self.base_path.join(collection);
        std::fs::create_dir_all(&buffer_dir)?;

        // Create TantivyStorageAdapter for unified storage
        let directory = TantivyStorageAdapter::new(
            self.storage.clone(),
            collection.to_string(),
            "default".to_string(),
            buffer_dir,
        ).map_err(|e| Error::Storage(e.to_string()))?;

        // Check if index exists by looking for meta.json file (Tantivy always creates this)
        use tantivy::directory::Directory;
        let meta_path = std::path::Path::new("meta.json");
        let index_exists = directory.exists(meta_path).unwrap_or(false);

        let index = if !index_exists {
            Index::create(
                directory,
                tantivy_schema.clone(),
                tantivy::IndexSettings::default(),
            )?
        } else {
            Index::open(directory)?
        };

        // Use the index's schema (may differ if opening existing index)
        let existing_schema = index.schema();
        let mut existing_field_map: HashMap<String, Field> = HashMap::new();
        for (field, entry) in existing_schema.fields() {
            existing_field_map.insert(entry.name().to_string(), field);
        }

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let writer = Arc::new(parking_lot::Mutex::new(index.writer(50_000_000)?));

        // Check if system fields exist in the loaded schema
        let indexed_at_enabled = existing_field_map.contains_key("_indexed_at");
        let boost_enabled = existing_field_map.contains_key("_boost");

        let collection_index = CollectionIndex {
            index,
            schema: existing_schema,
            field_map: existing_field_map,
            reader,
            writer,
            indexed_at_enabled,
            boost_enabled,
        };

        self.collections
            .write()
            .unwrap()
            .insert(collection.to_string(), collection_index);

        Ok(())
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

        // Get current timestamp for _indexed_at
        let now = DateTime::from_timestamp_micros(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros() as i64
        );

        for doc in docs {
            let mut tantivy_doc = TantivyDocument::new();

            // Add ID
            let id_field = coll.field_map.get("id").unwrap();
            tantivy_doc.add_text(*id_field, &doc.id);

            // Add system field: _indexed_at (auto-injected timestamp)
            if coll.indexed_at_enabled {
                if let Some(field) = coll.field_map.get("_indexed_at") {
                    tantivy_doc.add_date(*field, now);
                }
            }

            // Add system field: _boost (from document or default 1.0)
            if coll.boost_enabled {
                if let Some(field) = coll.field_map.get("_boost") {
                    let boost_value = doc.fields
                        .get("_boost")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0);
                    tantivy_doc.add_f64(*field, boost_value);
                }
            }

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

                        // Booleans
                        (serde_json::Value::Bool(b), tantivy::schema::FieldType::Bool(_)) => {
                            tantivy_doc.add_bool(*field, *b);
                            true
                        }

                        _ => false,
                    };

                    if !added {
                        tracing::warn!(
                            "Document '{}': skipped field '{}' with value {:?}",
                            doc.id,
                            field_name,
                            value
                        );
                    }
                }
            }

            writer.add_document(tantivy_doc)?;
        }

        writer.commit()?;

        // Reload reader to ensure newly committed documents are visible
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

        // Get searchable fields
        let mut searchable_fields = Vec::new();
        for (field, entry) in coll.schema.fields() {
            if entry.field_type().is_indexed() {
                if let tantivy::schema::FieldType::Str(_) = entry.field_type() {
                    searchable_fields.push(field);
                }
            }
        }

        // Determine fields to search
        let fields_to_search: Vec<Field> = if query.fields.is_empty() {
            searchable_fields
        } else {
            query
                .fields
                .iter()
                .filter_map(|f| coll.field_map.get(f).copied())
                .collect()
        };

        if fields_to_search.is_empty() {
            return Ok(SearchResults {
                results: vec![],
                total: 0,
                latency_ms: start.elapsed().as_millis() as u64,
            });
        }

        let query_parser = QueryParser::for_index(&coll.index, fields_to_search);
        let parsed_query = query_parser
            .parse_query(&query.query_string)
            .map_err(|e| Error::InvalidQuery(e.to_string()))?;

        let top_docs = searcher.search(&parsed_query, &TopDocs::with_limit(query.limit + query.offset))?;

        let id_field = coll.field_map.get("id").unwrap();
        let mut results = Vec::new();

        for (rank, (score, doc_addr)) in top_docs.iter().enumerate() {
            if rank < query.offset {
                continue;
            }

            let doc: TantivyDocument = searcher.doc(*doc_addr)?;

            // Get ID
            let id = doc
                .get_first(*id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Get all stored fields
            let mut fields = HashMap::new();
            for (field, entry) in coll.schema.fields() {
                if entry.is_stored() {
                    if let Some(value) = doc.get_first(field) {
                        let json_value = match value {
                            tantivy::schema::OwnedValue::Str(s) => serde_json::Value::String(s.to_string()),
                            tantivy::schema::OwnedValue::U64(n) => serde_json::Value::Number((*n).into()),
                            tantivy::schema::OwnedValue::I64(n) => serde_json::Value::Number((*n).into()),
                            tantivy::schema::OwnedValue::F64(n) => {
                                serde_json::Number::from_f64(*n)
                                    .map(serde_json::Value::Number)
                                    .unwrap_or(serde_json::Value::Null)
                            }
                            tantivy::schema::OwnedValue::Bool(b) => serde_json::Value::Bool(*b),
                            _ => continue,
                        };
                        fields.insert(entry.name().to_string(), json_value);
                    }
                }
            }

            results.push(SearchResult {
                id,
                score: *score,
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
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let searcher = coll.reader.searcher();
        let id_field = coll.field_map.get("id").unwrap();

        let term = Term::from_field_text(*id_field, id);
        let query = tantivy::query::TermQuery::new(term, IndexRecordOption::Basic);
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;

        if let Some((_score, doc_addr)) = top_docs.first() {
            let doc: TantivyDocument = searcher.doc(*doc_addr)?;

            let mut fields = HashMap::new();
            for (field, entry) in coll.schema.fields() {
                if entry.is_stored() {
                    if let Some(value) = doc.get_first(field) {
                        let json_value = match value {
                            tantivy::schema::OwnedValue::Str(s) => serde_json::Value::String(s.to_string()),
                            tantivy::schema::OwnedValue::U64(n) => serde_json::Value::Number((*n).into()),
                            tantivy::schema::OwnedValue::I64(n) => serde_json::Value::Number((*n).into()),
                            tantivy::schema::OwnedValue::F64(n) => {
                                serde_json::Number::from_f64(*n)
                                    .map(serde_json::Value::Number)
                                    .unwrap_or(serde_json::Value::Null)
                            }
                            tantivy::schema::OwnedValue::Bool(b) => serde_json::Value::Bool(*b),
                            _ => continue,
                        };
                        fields.insert(entry.name().to_string(), json_value);
                    }
                }
            }

            Ok(Some(Document {
                id: id.to_string(),
                fields,
            }))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, collection: &str, ids: Vec<String>) -> Result<()> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let id_field = coll.field_map.get("id").unwrap();
        let mut writer = coll.writer.lock();

        for id in ids {
            let term = Term::from_field_text(*id_field, &id);
            writer.delete_term(term);
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
        let segment_readers = searcher.segment_readers();

        let document_count: usize = segment_readers.iter().map(|r| r.num_docs() as usize).sum();
        let size_bytes: usize = segment_readers.iter().map(|r| r.num_deleted_docs() as usize * 100).sum();

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
        let start = std::time::Instant::now();

        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let searcher = coll.reader.searcher();

        // Get searchable text fields
        let mut searchable_fields = Vec::new();
        for (field, entry) in coll.schema.fields() {
            if entry.field_type().is_indexed() {
                if let tantivy::schema::FieldType::Str(_) = entry.field_type() {
                    searchable_fields.push(field);
                }
            }
        }

        let fields_to_search: Vec<Field> = if query.fields.is_empty() {
            searchable_fields
        } else {
            query
                .fields
                .iter()
                .filter_map(|f| coll.field_map.get(f).copied())
                .collect()
        };

        if fields_to_search.is_empty() {
            return Ok(SearchResultsWithAggs {
                results: vec![],
                total: 0,
                aggregations: HashMap::new(),
            });
        }

        let query_parser = QueryParser::for_index(&coll.index, fields_to_search);
        let parsed_query = query_parser
            .parse_query(&query.query_string)
            .map_err(|e| Error::InvalidQuery(e.to_string()))?;

        // Collect all matching docs for aggregations
        let all_docs = searcher.search(&parsed_query, &TopDocs::with_limit(10000))?;

        // Build results
        let id_field = coll.field_map.get("id").unwrap();
        let mut results = Vec::new();

        for (rank, (score, doc_addr)) in all_docs.iter().enumerate() {
            if rank < query.offset || rank >= query.offset + query.limit {
                continue;
            }

            let doc: TantivyDocument = searcher.doc(*doc_addr)?;

            let id = doc
                .get_first(*id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let mut fields = HashMap::new();
            for (field, entry) in coll.schema.fields() {
                if entry.is_stored() {
                    if let Some(value) = doc.get_first(field) {
                        let json_value = match value {
                            tantivy::schema::OwnedValue::Str(s) => serde_json::Value::String(s.to_string()),
                            tantivy::schema::OwnedValue::U64(n) => serde_json::Value::Number((*n).into()),
                            tantivy::schema::OwnedValue::I64(n) => serde_json::Value::Number((*n).into()),
                            tantivy::schema::OwnedValue::F64(n) => {
                                serde_json::Number::from_f64(*n)
                                    .map(serde_json::Value::Number)
                                    .unwrap_or(serde_json::Value::Null)
                            }
                            tantivy::schema::OwnedValue::Bool(b) => serde_json::Value::Bool(*b),
                            _ => continue,
                        };
                        fields.insert(entry.name().to_string(), json_value);
                    }
                }
            }

            results.push(SearchResult {
                id,
                score: *score,
                fields,
            });
        }

        // Run aggregations using simple value-based computation
        let mut agg_results: HashMap<String, AggregationResult> = HashMap::new();

        for agg_req in aggregations {
            // Extract field name from aggregation type
            let field_name = match &agg_req.agg_type {
                AggregationType::Count => None,
                AggregationType::Sum { field } => Some(field.clone()),
                AggregationType::Avg { field } => Some(field.clone()),
                AggregationType::Min { field } => Some(field.clone()),
                AggregationType::Max { field } => Some(field.clone()),
                AggregationType::Stats { field } => Some(field.clone()),
                AggregationType::Terms { field, .. } => Some(field.clone()),
                AggregationType::Histogram { field, .. } => Some(field.clone()),
                AggregationType::DateHistogram { field, .. } => Some(field.clone()),
            };

            // Count doesn't need a field
            if matches!(agg_req.agg_type, AggregationType::Count) {
                agg_results.insert(
                    agg_req.name.clone(),
                    AggregationResult {
                        name: agg_req.name.clone(),
                        value: AggregationValue::Single(all_docs.len() as f64),
                    },
                );
                continue;
            }

            let field_name = match field_name {
                Some(f) => f,
                None => continue,
            };

            let field = match coll.field_map.get(&field_name) {
                Some(f) => *f,
                None => continue,
            };

            // Collect numeric values from all matching documents
            let mut numeric_values: Vec<f64> = Vec::new();
            let mut string_values: Vec<String> = Vec::new();

            for (_score, doc_addr) in &all_docs {
                let doc: TantivyDocument = searcher.doc(*doc_addr)?;
                if let Some(value) = doc.get_first(field) {
                    match value {
                        tantivy::schema::OwnedValue::U64(n) => numeric_values.push(*n as f64),
                        tantivy::schema::OwnedValue::I64(n) => numeric_values.push(*n as f64),
                        tantivy::schema::OwnedValue::F64(n) => numeric_values.push(*n),
                        tantivy::schema::OwnedValue::Str(s) => string_values.push(s.to_string()),
                        _ => {}
                    }
                }
            }

            let agg_value = match &agg_req.agg_type {
                AggregationType::Sum { .. } => {
                    let sum: f64 = numeric_values.iter().sum();
                    AggregationValue::Single(sum)
                }
                AggregationType::Avg { .. } => {
                    let avg = if numeric_values.is_empty() {
                        0.0
                    } else {
                        numeric_values.iter().sum::<f64>() / numeric_values.len() as f64
                    };
                    AggregationValue::Single(avg)
                }
                AggregationType::Min { .. } => {
                    let min = numeric_values.iter().cloned().fold(f64::INFINITY, f64::min);
                    AggregationValue::Single(if min.is_infinite() { 0.0 } else { min })
                }
                AggregationType::Max { .. } => {
                    let max = numeric_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    AggregationValue::Single(if max.is_infinite() { 0.0 } else { max })
                }
                AggregationType::Stats { .. } => {
                    let count = numeric_values.len() as u64;
                    let sum: f64 = numeric_values.iter().sum();
                    let min = numeric_values.iter().cloned().fold(f64::INFINITY, f64::min);
                    let max = numeric_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    let avg = if count == 0 { None } else { Some(sum / count as f64) };
                    AggregationValue::Stats(StatsResult {
                        count,
                        min: if min.is_infinite() { None } else { Some(min) },
                        max: if max.is_infinite() { None } else { Some(max) },
                        sum: Some(sum),
                        avg,
                    })
                }
                AggregationType::Terms { size, .. } => {
                    let mut counts: HashMap<String, u64> = HashMap::new();
                    for s in &string_values {
                        *counts.entry(s.clone()).or_insert(0) += 1;
                    }
                    // Also count numeric values as strings
                    for n in &numeric_values {
                        *counts.entry(n.to_string()).or_insert(0) += 1;
                    }
                    let mut bucket_vec: Vec<_> = counts.into_iter().collect();
                    bucket_vec.sort_by(|a, b| b.1.cmp(&a.1));
                    let size = size.unwrap_or(10);
                    let buckets: Vec<Bucket> = bucket_vec
                        .into_iter()
                        .take(size)
                        .map(|(key, doc_count)| Bucket {
                            key,
                            doc_count,
                            sub_aggs: None,
                        })
                        .collect();
                    AggregationValue::Buckets(buckets)
                }
                _ => AggregationValue::Single(0.0),
            };

            agg_results.insert(
                agg_req.name.clone(),
                AggregationResult {
                    name: agg_req.name.clone(),
                    value: agg_value,
                },
            );
        }

        let total = all_docs.len() as u64;

        Ok(SearchResultsWithAggs {
            results,
            total,
            aggregations: agg_results,
        })
    }
}

// ============================================================================
// Index Inspection API (Issue #24)
// ============================================================================

/// Term information for index inspection
#[derive(Debug, Clone, serde::Serialize)]
pub struct TermInfo {
    pub term: String,
    pub doc_freq: u64,
}

/// Segment information for index inspection
#[derive(Debug, Clone, serde::Serialize)]
pub struct SegmentInfo {
    pub id: String,
    pub doc_count: u32,
    pub deleted_count: u32,
    pub size_bytes: u64,
}

/// Segments overview response
#[derive(Debug, Clone, serde::Serialize)]
pub struct SegmentsInfo {
    pub segments: Vec<SegmentInfo>,
    pub total_docs: u64,
    pub total_deleted: u64,
    pub delete_ratio: f64,
}

/// Reconstructed document with indexed terms
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReconstructedDocument {
    pub id: String,
    pub stored_fields: HashMap<String, serde_json::Value>,
    pub indexed_terms: HashMap<String, Vec<String>>,
}

impl TextBackend {
    /// Get top-k most frequent terms for a field.
    pub fn get_top_terms(&self, collection: &str, field: &str, limit: usize) -> Result<Vec<TermInfo>> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let field_obj = coll.field_map.get(field)
            .ok_or_else(|| Error::Schema(format!("Field '{}' not found", field)))?;

        let searcher = coll.reader.searcher();
        let mut term_counts: HashMap<String, u64> = HashMap::new();

        // Iterate through all segments
        for segment_reader in searcher.segment_readers() {
            let inverted_index = segment_reader.inverted_index(*field_obj)?;
            let term_dict = inverted_index.terms();
            let mut term_stream = term_dict.stream()?;

            while term_stream.advance() {
                let term_bytes = term_stream.key();
                if let Ok(term_str) = std::str::from_utf8(term_bytes) {
                    let doc_freq = term_stream.value().doc_freq as u64;
                    *term_counts.entry(term_str.to_string()).or_insert(0) += doc_freq;
                }
            }
        }

        // Sort by frequency and take top-k
        let mut terms: Vec<_> = term_counts.into_iter().collect();
        terms.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(terms
            .into_iter()
            .take(limit)
            .map(|(term, doc_freq)| TermInfo { term, doc_freq })
            .collect())
    }

    /// Get segment information for a collection.
    pub fn get_segments(&self, collection: &str) -> Result<SegmentsInfo> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let searcher = coll.reader.searcher();
        let mut segments = Vec::new();
        let mut total_docs: u64 = 0;
        let mut total_deleted: u64 = 0;

        for segment_reader in searcher.segment_readers() {
            let segment_id = segment_reader.segment_id();
            let doc_count = segment_reader.num_docs();
            let deleted_count = segment_reader.num_deleted_docs();

            // Estimate size from segment ordinal (actual size requires file system access)
            let size_bytes = (doc_count as u64) * 500; // Rough estimate

            segments.push(SegmentInfo {
                id: format!("{:?}", segment_id),
                doc_count,
                deleted_count,
                size_bytes,
            });

            total_docs += doc_count as u64;
            total_deleted += deleted_count as u64;
        }

        let delete_ratio = if total_docs > 0 {
            total_deleted as f64 / (total_docs + total_deleted) as f64
        } else {
            0.0
        };

        Ok(SegmentsInfo {
            segments,
            total_docs,
            total_deleted,
            delete_ratio,
        })
    }

    /// Reconstruct a document showing stored fields and indexed terms.
    pub fn reconstruct_document(&self, collection: &str, id: &str) -> Result<Option<ReconstructedDocument>> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let searcher = coll.reader.searcher();
        let id_field = coll.field_map.get("id").unwrap();

        // Find document by ID
        let term = Term::from_field_text(*id_field, id);
        let query = tantivy::query::TermQuery::new(term, IndexRecordOption::Basic);
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;

        let (_, doc_addr) = match top_docs.first() {
            Some(d) => d,
            None => return Ok(None),
        };

        let doc: TantivyDocument = searcher.doc(*doc_addr)?;

        // Collect stored fields
        let mut stored_fields = HashMap::new();
        for (field, entry) in coll.schema.fields() {
            if entry.is_stored() {
                if let Some(value) = doc.get_first(field) {
                    let json_value = match value {
                        tantivy::schema::OwnedValue::Str(s) => serde_json::Value::String(s.to_string()),
                        tantivy::schema::OwnedValue::U64(n) => serde_json::Value::Number((*n).into()),
                        tantivy::schema::OwnedValue::I64(n) => serde_json::Value::Number((*n).into()),
                        tantivy::schema::OwnedValue::F64(n) => {
                            serde_json::Number::from_f64(*n)
                                .map(serde_json::Value::Number)
                                .unwrap_or(serde_json::Value::Null)
                        }
                        tantivy::schema::OwnedValue::Bool(b) => serde_json::Value::Bool(*b),
                        _ => continue,
                    };
                    stored_fields.insert(entry.name().to_string(), json_value);
                }
            }
        }

        // Collect indexed terms for text fields
        let mut indexed_terms: HashMap<String, Vec<String>> = HashMap::new();

        // Get the segment reader for this document
        let segment_ord = doc_addr.segment_ord;
        let segment_reader = &searcher.segment_readers()[segment_ord as usize];
        let doc_id = doc_addr.doc_id;

        for (field, entry) in coll.schema.fields() {
            if entry.field_type().is_indexed() {
                if let tantivy::schema::FieldType::Str(_) = entry.field_type() {
                    let mut terms_for_field = Vec::new();

                    // Get inverted index for this field
                    if let Ok(inverted_index) = segment_reader.inverted_index(field) {
                        let term_dict = inverted_index.terms();
                        let mut term_stream = term_dict.stream()?;

                        while term_stream.advance() {
                            let term_bytes = term_stream.key();
                            if let Ok(postings) = inverted_index.read_postings_from_terminfo(
                                &term_stream.value(),
                                IndexRecordOption::Basic,
                            ) {
                                // Check if this document contains this term
                                let mut postings = postings;
                                while postings.doc() < doc_id {
                                    if postings.advance() == tantivy::TERMINATED {
                                        break;
                                    }
                                }
                                if postings.doc() == doc_id {
                                    if let Ok(term_str) = std::str::from_utf8(term_bytes) {
                                        terms_for_field.push(term_str.to_string());
                                    }
                                }
                            }
                        }
                    }

                    if !terms_for_field.is_empty() {
                        indexed_terms.insert(entry.name().to_string(), terms_for_field);
                    }
                }
            }
        }

        Ok(Some(ReconstructedDocument {
            id: id.to_string(),
            stored_fields,
            indexed_terms,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_text_backend_with_segment_storage() {
        let dir = tempdir().unwrap();
        let storage = Arc::new(LocalStorage::new(dir.path()));
        let backend = TextBackend::with_segment_storage(dir.path(), storage).unwrap();

        // Backend should be created successfully
        assert!(backend.collections.read().unwrap().is_empty());
    }
}
