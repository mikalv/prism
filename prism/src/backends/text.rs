//! Text search backend with unified SegmentStorage integration
//!
//! All storage (local, S3, cached) goes through the SegmentStorage trait via TantivyStorageAdapter.

use crate::aggregations::types::StatsResult;
use crate::aggregations::{
    AggregationRequest, AggregationResult, AggregationType, AggregationValue, Bucket,
};
use crate::backends::{
    BackendStats, Document, Query, SearchBackend, SearchResult, SearchResults,
    SearchResultsWithAggs,
};
use crate::ranking::{apply_ranking_adjustments, RankableResult, RankingConfig};
use crate::schema::{CollectionSchema, FieldType, TokenizerType};
use crate::tokenizer::{code_tokenizer, CODE_TOKENIZER_NAME};
use crate::{Error, Result};
use async_trait::async_trait;
use prism_storage::{LocalStorage, SegmentStorage, TantivyStorageAdapter};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tantivy::{
    collector::TopDocs, indexer::NoMergePolicy, query::QueryParser, schema::*, DateTime, DocSet,
    Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term,
};

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
    /// Boosting configuration for ranking adjustments
    boosting_config: Option<crate::schema::BoostingConfig>,
}

/// Convert a Tantivy OwnedValue to a serde_json::Value.
/// Handles all stored types: Str, U64, I64, F64, Bool, Date (→ ISO 8601), Bytes (→ base64).
fn owned_value_to_json(value: &tantivy::schema::OwnedValue) -> Option<serde_json::Value> {
    match value {
        tantivy::schema::OwnedValue::Str(s) => Some(serde_json::Value::String(s.to_string())),
        tantivy::schema::OwnedValue::U64(n) => Some(serde_json::Value::Number((*n).into())),
        tantivy::schema::OwnedValue::I64(n) => Some(serde_json::Value::Number((*n).into())),
        tantivy::schema::OwnedValue::F64(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .or(Some(serde_json::Value::Null)),
        tantivy::schema::OwnedValue::Bool(b) => Some(serde_json::Value::Bool(*b)),
        tantivy::schema::OwnedValue::Date(dt) => {
            let micros = dt.into_timestamp_micros();
            let secs = micros / 1_000_000;
            let nsecs = ((micros % 1_000_000) * 1000) as u32;
            if let Some(ndt) = chrono::DateTime::from_timestamp(secs, nsecs) {
                Some(serde_json::Value::String(
                    ndt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                ))
            } else {
                Some(serde_json::Value::Number(micros.into()))
            }
        }
        tantivy::schema::OwnedValue::Bytes(b) => Some(serde_json::Value::String(
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b),
        )),
        _ => None,
    }
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

    /// Remove a collection from this backend, dropping all in-memory state.
    pub fn remove_collection(&self, name: &str) {
        self.collections.write().unwrap().remove(name);
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
                NumericOptions::default()
                    .set_indexed()
                    .set_stored()
                    .set_fast(),
            );
            field_map.insert("_boost".to_string(), boost_field);
        }

        // Add configured fields
        for field_def in &text_config.fields {
            let field = match field_def.field_type {
                FieldType::Text => {
                    let mut options = TextOptions::default();
                    if field_def.indexed {
                        // Select tokenizer based on field configuration
                        let tokenizer_name = match field_def
                            .tokenizer
                            .as_ref()
                            .unwrap_or(&TokenizerType::Default)
                        {
                            TokenizerType::Default => "default",
                            TokenizerType::Code => CODE_TOKENIZER_NAME,
                            TokenizerType::Raw => "raw",
                            TokenizerType::CodeTreeSitter => {
                                #[cfg(feature = "tokenizer-treesitter")]
                                {
                                    let lang = field_def
                                        .tokenizer_options
                                        .as_ref()
                                        .and_then(|o| o.language.as_deref());
                                    match lang {
                                        Some(l) => {
                                            // Leak a string to get a &'static str for the match
                                            // This is fine: called once at init, small set of languages
                                            let name = format!("code-treesitter-{}", l);
                                            Box::leak(name.into_boxed_str()) as &str
                                        }
                                        None => "code-treesitter",
                                    }
                                }
                                #[cfg(not(feature = "tokenizer-treesitter"))]
                                {
                                    tracing::warn!(
                                        "code-treesitter tokenizer requested for field '{}' but \
                                        tokenizer-treesitter feature is not enabled; falling back to code tokenizer",
                                        field_def.name
                                    );
                                    CODE_TOKENIZER_NAME
                                }
                            }
                        };
                        options = options.set_indexing_options(
                            TextFieldIndexing::default()
                                .set_tokenizer(tokenizer_name)
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
        )
        .map_err(|e| Error::Storage(e.to_string()))?;

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

        // Register custom tokenizers
        index
            .tokenizers()
            .register(CODE_TOKENIZER_NAME, code_tokenizer());

        // Register tree-sitter tokenizers if feature is enabled
        #[cfg(feature = "tokenizer-treesitter")]
        prism_treesitter::register_tokenizers(index.tokenizers());

        // Use the index's schema (may differ if opening existing index)
        let existing_schema = index.schema();
        let mut existing_field_map: HashMap<String, Field> = HashMap::new();
        for (field, entry) in existing_schema.fields() {
            existing_field_map.insert(entry.name().to_string(), field);
        }

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        let writer = index.writer(50_000_000)?;
        // Disable background merge threads. Our TantivyStorageAdapter doesn't
        // support Unix unlink semantics (deleted files remaining accessible via
        // open file handles), so background merges that delete old segment files
        // cause "Path not found" crashes in concurrent readers/writers.
        writer.set_merge_policy(Box::new(NoMergePolicy));
        let writer = Arc::new(parking_lot::Mutex::new(writer));

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
            boosting_config: schema.boosting.clone(),
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
    #[tracing::instrument(name = "text_index", skip(self, docs), fields(collection = %collection, doc_count = docs.len()))]
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
                .as_micros() as i64,
        );

        for doc in docs {
            let mut tantivy_doc = TantivyDocument::new();

            // Add ID
            let id_field = coll.field_map.get("id").unwrap();

            // Upsert: delete existing document with same ID before adding new one.
            // This ensures re-indexing replaces rather than duplicates.
            let delete_term = Term::from_field_text(*id_field, &doc.id);
            writer.delete_term(delete_term);

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
                    let boost_value = doc
                        .fields
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
                        // Date fields — parse ISO 8601 string or epoch micros
                        (serde_json::Value::String(s), tantivy::schema::FieldType::Date(_)) => {
                            // Try RFC 3339 / ISO 8601 parsing via chrono
                            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                                let micros = dt.timestamp_micros();
                                tantivy_doc
                                    .add_date(*field, DateTime::from_timestamp_micros(micros));
                                true
                            } else {
                                tracing::warn!(
                                    "Document '{}': could not parse date string '{}' for field '{}'",
                                    doc.id, s, field_name
                                );
                                false
                            }
                        }
                        (serde_json::Value::Number(n), tantivy::schema::FieldType::Date(_)) => {
                            // Treat number as epoch microseconds
                            if let Some(micros) = n.as_i64() {
                                tantivy_doc
                                    .add_date(*field, DateTime::from_timestamp_micros(micros));
                                true
                            } else {
                                false
                            }
                        }

                        // String/text values (must come AFTER Date match)
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

    #[tracing::instrument(name = "text_search", skip(self, query), fields(collection = %collection))]
    async fn search(&self, collection: &str, query: Query) -> Result<SearchResults> {
        let start = std::time::Instant::now();

        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        // Reload reader to pick up any background merges that may have
        // deleted old segment files. Without this, a stale reader can
        // reference segments that the merge thread already garbage-collected.
        coll.reader.reload()?;
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

        let mut query_parser = QueryParser::for_index(&coll.index, fields_to_search.clone());

        // Tantivy's query parser can panic on certain inputs (e.g., bare `*`
        // triggers "Exist query without a field isn't allowed").  Catch panics
        // so malicious/malformed queries don't crash the server.
        let query_string = query.query_string.clone();
        let parsed_query = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            query_parser.parse_query(&query_string)
        })) {
            Ok(Ok(q)) => q,
            Ok(Err(e)) => return Err(Error::InvalidQuery(e.to_string())),
            Err(_) => {
                return Err(Error::InvalidQuery(format!(
                    "Query parser panicked on input: {:?}",
                    query_string
                )));
            }
        };

        let top_docs = searcher.search(
            &parsed_query,
            &TopDocs::with_limit(query.limit + query.offset),
        )?;

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
                        if let Some(json_value) = owned_value_to_json(value) {
                            fields.insert(entry.name().to_string(), json_value);
                        }
                    }
                }
            }

            results.push(SearchResult {
                id,
                score: *score,
                fields,
                highlight: None,
            });
        }

        // Generate highlights if requested
        if let Some(ref hl_config) = query.highlight {
            use tantivy::snippet::SnippetGenerator;

            for hl_field_name in &hl_config.fields {
                if let Some(&field) = coll.field_map.get(hl_field_name) {
                    // Only generate snippets for text fields
                    let entry = coll.schema.get_field_entry(field);
                    if !matches!(entry.field_type(), tantivy::schema::FieldType::Str(_)) {
                        continue;
                    }

                    if let Ok(mut generator) =
                        SnippetGenerator::create(&searcher, &*parsed_query, field)
                    {
                        generator.set_max_num_chars(hl_config.fragment_size);

                        for result in &mut results {
                            // Get the stored text for this field
                            if let Some(text_value) =
                                result.fields.get(hl_field_name).and_then(|v| v.as_str())
                            {
                                let mut snippet = generator.snippet(text_value);
                                if !snippet.is_empty() {
                                    snippet.set_snippet_prefix_postfix(
                                        &hl_config.pre_tag,
                                        &hl_config.post_tag,
                                    );
                                    let html = snippet.to_html();

                                    let highlights =
                                        result.highlight.get_or_insert_with(HashMap::new);
                                    let fragments = highlights
                                        .entry(hl_field_name.clone())
                                        .or_insert_with(Vec::new);
                                    fragments.push(html);
                                    // Tantivy returns one best fragment per call; truncate to configured max
                                    fragments.truncate(hl_config.number_of_fragments);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Apply ranking adjustments if boosting is configured
        let results = if let Some(boosting_config) = &coll.boosting_config {
            let ranking_config = RankingConfig::from_boosting_config(boosting_config);
            let now = std::time::SystemTime::now();

            // Preserve highlights before moving results into ranking pipeline
            let highlight_map: HashMap<String, Option<HashMap<String, Vec<String>>>> = results
                .iter()
                .map(|r| (r.id.clone(), r.highlight.clone()))
                .collect();

            // Convert to rankable results
            let mut rankable: Vec<RankableResult> = results
                .into_iter()
                .map(|r| RankableResult::from_fields(r.id, r.score, r.fields))
                .collect();

            // Apply ranking adjustments (recency decay, popularity boost)
            apply_ranking_adjustments(&mut rankable, &ranking_config, now);

            // Convert back to SearchResult, restoring highlights
            rankable
                .into_iter()
                .map(|r| {
                    let hl = highlight_map.get(&r.id).cloned().flatten();
                    SearchResult {
                        id: r.id,
                        score: r.adjusted_score,
                        fields: r.fields,
                        highlight: hl,
                    }
                })
                .collect()
        } else {
            results
        };

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

        coll.reader.reload()?;
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
                        if let Some(json_value) = owned_value_to_json(value) {
                            fields.insert(entry.name().to_string(), json_value);
                        }
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

        coll.reader.reload()?;
        let searcher = coll.reader.searcher();
        let segment_readers = searcher.segment_readers();

        let document_count: usize = segment_readers.iter().map(|r| r.num_docs() as usize).sum();
        let size_bytes: usize = segment_readers
            .iter()
            .map(|r| r.num_deleted_docs() as usize * 100)
            .sum();

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
        let _start = std::time::Instant::now();

        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        coll.reader.reload()?;
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
            searchable_fields.clone()
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

        let query_parser = QueryParser::for_index(&coll.index, fields_to_search.clone());
        let query_string = query.query_string.clone();
        let parsed_query = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            query_parser.parse_query(&query_string)
        })) {
            Ok(Ok(q)) => q,
            Ok(Err(e)) => return Err(Error::InvalidQuery(e.to_string())),
            Err(_) => {
                return Err(Error::InvalidQuery(format!(
                    "Query parser panicked on input: {:?}",
                    query_string
                )));
            }
        };

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
                        if let Some(json_value) = owned_value_to_json(value) {
                            fields.insert(entry.name().to_string(), json_value);
                        }
                    }
                }
            }

            results.push(SearchResult {
                id,
                score: *score,
                fields,
                highlight: None,
            });
        }

        // Collect all doc addresses for aggregation processing
        let doc_addrs: Vec<tantivy::DocAddress> = all_docs.iter().map(|(_s, addr)| *addr).collect();

        // Run aggregations
        let agg_results = execute_aggregations(
            &searcher,
            coll,
            &searchable_fields,
            &doc_addrs,
            &aggregations,
        )?;

        let total = all_docs.len() as u64;

        Ok(SearchResultsWithAggs {
            results,
            total,
            aggregations: agg_results,
        })
    }
}

// ============================================================================
// Aggregation execution engine
// ============================================================================

use crate::aggregations::types::{HistogramBounds, PercentilesResult, RangeEntry};

/// Collect field values from a set of document addresses
fn collect_field_values(
    searcher: &tantivy::Searcher,
    field: Field,
    doc_addrs: &[tantivy::DocAddress],
) -> Result<(Vec<f64>, Vec<String>)> {
    let mut numeric_values = Vec::new();
    let mut string_values = Vec::new();

    for doc_addr in doc_addrs {
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

    Ok((numeric_values, string_values))
}

/// Compute percentile value from a sorted array using linear interpolation
fn compute_percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }
    let rank = (p / 100.0) * (sorted.len() as f64 - 1.0);
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper || upper >= sorted.len() {
        Some(sorted[lower.min(sorted.len() - 1)])
    } else {
        let frac = rank - lower as f64;
        Some(sorted[lower] * (1.0 - frac) + sorted[upper] * frac)
    }
}

/// Collect per-bucket document addresses keyed by bucket key, for sub-aggregation
fn collect_docs_per_bucket_terms(
    searcher: &tantivy::Searcher,
    field: Field,
    doc_addrs: &[tantivy::DocAddress],
) -> Result<HashMap<String, Vec<tantivy::DocAddress>>> {
    let mut map: HashMap<String, Vec<tantivy::DocAddress>> = HashMap::new();
    for &doc_addr in doc_addrs {
        let doc: TantivyDocument = searcher.doc(doc_addr)?;
        if let Some(value) = doc.get_first(field) {
            let key = match value {
                tantivy::schema::OwnedValue::Str(s) => s.to_string(),
                tantivy::schema::OwnedValue::U64(n) => n.to_string(),
                tantivy::schema::OwnedValue::I64(n) => n.to_string(),
                tantivy::schema::OwnedValue::F64(n) => n.to_string(),
                tantivy::schema::OwnedValue::Bool(b) => b.to_string(),
                tantivy::schema::OwnedValue::Date(dt) => {
                    let micros = dt.into_timestamp_micros();
                    let secs = micros / 1_000_000;
                    let nsecs = ((micros % 1_000_000) * 1000) as u32;
                    chrono::DateTime::from_timestamp(secs, nsecs)
                        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                        .unwrap_or_else(|| micros.to_string())
                }
                _ => continue,
            };
            map.entry(key).or_default().push(doc_addr);
        }
    }
    Ok(map)
}

/// Execute a list of aggregation requests against a set of document addresses
fn execute_aggregations(
    searcher: &tantivy::Searcher,
    coll: &CollectionIndex,
    searchable_fields: &[Field],
    doc_addrs: &[tantivy::DocAddress],
    aggregations: &[AggregationRequest],
) -> Result<HashMap<String, AggregationResult>> {
    let mut agg_results: HashMap<String, AggregationResult> = HashMap::new();

    for agg_req in aggregations {
        let result = execute_single_agg(searcher, coll, searchable_fields, doc_addrs, agg_req)?;
        agg_results.insert(agg_req.name.clone(), result);
    }

    Ok(agg_results)
}

/// Execute a single aggregation request
fn execute_single_agg(
    searcher: &tantivy::Searcher,
    coll: &CollectionIndex,
    searchable_fields: &[Field],
    doc_addrs: &[tantivy::DocAddress],
    agg_req: &AggregationRequest,
) -> Result<AggregationResult> {
    let sub_aggs = agg_req.aggs.as_deref().unwrap_or(&[]);

    let value = match &agg_req.agg_type {
        AggregationType::Count => AggregationValue::Single(doc_addrs.len() as f64),

        AggregationType::Sum { field } => {
            let f = resolve_field(coll, field)?;
            let (nums, _) = collect_field_values(searcher, f, doc_addrs)?;
            AggregationValue::Single(nums.iter().sum())
        }

        AggregationType::Avg { field } => {
            let f = resolve_field(coll, field)?;
            let (nums, _) = collect_field_values(searcher, f, doc_addrs)?;
            let avg = if nums.is_empty() {
                0.0
            } else {
                nums.iter().sum::<f64>() / nums.len() as f64
            };
            AggregationValue::Single(avg)
        }

        AggregationType::Min { field } => {
            let f = resolve_field(coll, field)?;
            let (nums, _) = collect_field_values(searcher, f, doc_addrs)?;
            let min = nums.iter().cloned().fold(f64::INFINITY, f64::min);
            AggregationValue::Single(if min.is_infinite() { 0.0 } else { min })
        }

        AggregationType::Max { field } => {
            let f = resolve_field(coll, field)?;
            let (nums, _) = collect_field_values(searcher, f, doc_addrs)?;
            let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            AggregationValue::Single(if max.is_infinite() { 0.0 } else { max })
        }

        AggregationType::Stats { field } => {
            let f = resolve_field(coll, field)?;
            let (nums, _) = collect_field_values(searcher, f, doc_addrs)?;
            let count = nums.len() as u64;
            let sum: f64 = nums.iter().sum();
            let min = nums.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let avg = if count == 0 {
                None
            } else {
                Some(sum / count as f64)
            };
            AggregationValue::Stats(StatsResult {
                count,
                min: if min.is_infinite() { None } else { Some(min) },
                max: if max.is_infinite() { None } else { Some(max) },
                sum: Some(sum),
                avg,
            })
        }

        AggregationType::Percentiles { field, percents } => {
            let f = resolve_field(coll, field)?;
            let (mut nums, _) = collect_field_values(searcher, f, doc_addrs)?;
            nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mut values = HashMap::new();
            for &p in percents {
                let key = format!("{}", p);
                values.insert(key, compute_percentile(&nums, p));
            }
            AggregationValue::Percentiles(PercentilesResult { values })
        }

        AggregationType::Terms { field, size } => {
            let f = resolve_field(coll, field)?;
            let size = size.unwrap_or(10);

            if sub_aggs.is_empty() {
                // Simple case: no sub-aggregations
                let (nums, strings) = collect_field_values(searcher, f, doc_addrs)?;
                let mut counts: HashMap<String, u64> = HashMap::new();
                for s in &strings {
                    *counts.entry(s.clone()).or_insert(0) += 1;
                }
                for n in &nums {
                    *counts.entry(n.to_string()).or_insert(0) += 1;
                }
                let mut bucket_vec: Vec<_> = counts.into_iter().collect();
                bucket_vec.sort_by(|a, b| b.1.cmp(&a.1));
                let buckets: Vec<Bucket> = bucket_vec
                    .into_iter()
                    .take(size)
                    .map(|(key, doc_count)| Bucket {
                        key,
                        doc_count,
                        from: None,
                        to: None,
                        sub_aggs: None,
                    })
                    .collect();
                AggregationValue::Buckets(buckets)
            } else {
                // With sub-aggregations: need per-bucket doc sets
                let docs_per_bucket = collect_docs_per_bucket_terms(searcher, f, doc_addrs)?;
                let mut bucket_vec: Vec<_> = docs_per_bucket
                    .iter()
                    .map(|(k, addrs)| (k.clone(), addrs.len() as u64, addrs.clone()))
                    .collect();
                bucket_vec.sort_by(|a, b| b.1.cmp(&a.1));
                bucket_vec.truncate(size);

                let mut buckets = Vec::new();
                for (key, doc_count, addrs) in bucket_vec {
                    let child_aggs =
                        execute_aggregations(searcher, coll, searchable_fields, &addrs, sub_aggs)?;
                    let child_vec: Vec<AggregationResult> = child_aggs.into_values().collect();
                    buckets.push(Bucket {
                        key,
                        doc_count,
                        from: None,
                        to: None,
                        sub_aggs: if child_vec.is_empty() {
                            None
                        } else {
                            Some(child_vec)
                        },
                    });
                }
                AggregationValue::Buckets(buckets)
            }
        }

        AggregationType::Histogram {
            field,
            interval,
            min_doc_count,
            extended_bounds,
        } => {
            let f = resolve_field(coll, field)?;
            let interval = *interval;

            if sub_aggs.is_empty() {
                let (nums, _) = collect_field_values(searcher, f, doc_addrs)?;
                let buckets = compute_histogram_buckets(
                    &nums,
                    interval,
                    *min_doc_count,
                    extended_bounds.as_ref(),
                );
                AggregationValue::Buckets(buckets)
            } else {
                // Per-bucket doc addresses for sub-aggs
                let mut bucket_docs: std::collections::BTreeMap<i64, Vec<tantivy::DocAddress>> =
                    std::collections::BTreeMap::new();
                for &doc_addr in doc_addrs {
                    let doc: TantivyDocument = searcher.doc(doc_addr)?;
                    if let Some(value) = doc.get_first(f) {
                        let v = match value {
                            tantivy::schema::OwnedValue::U64(n) => Some(*n as f64),
                            tantivy::schema::OwnedValue::I64(n) => Some(*n as f64),
                            tantivy::schema::OwnedValue::F64(n) => Some(*n),
                            _ => None,
                        };
                        if let Some(v) = v {
                            let bucket_key = (v / interval).floor() as i64;
                            bucket_docs.entry(bucket_key).or_default().push(doc_addr);
                        }
                    }
                }

                // Apply extended bounds
                if let Some(bounds) = extended_bounds {
                    let min_key = (bounds.min / interval).floor() as i64;
                    let max_key = (bounds.max / interval).floor() as i64;
                    for k in min_key..=max_key {
                        bucket_docs.entry(k).or_default();
                    }
                }

                let min_count = min_doc_count.unwrap_or(0);
                let mut buckets = Vec::new();
                for (bucket_key, addrs) in &bucket_docs {
                    let doc_count = addrs.len() as u64;
                    if doc_count < min_count {
                        continue;
                    }
                    let child_aggs =
                        execute_aggregations(searcher, coll, searchable_fields, addrs, sub_aggs)?;
                    let child_vec: Vec<AggregationResult> = child_aggs.into_values().collect();
                    buckets.push(Bucket {
                        key: format!("{}", (*bucket_key as f64) * interval),
                        doc_count,
                        from: None,
                        to: None,
                        sub_aggs: if child_vec.is_empty() {
                            None
                        } else {
                            Some(child_vec)
                        },
                    });
                }
                AggregationValue::Buckets(buckets)
            }
        }

        AggregationType::DateHistogram {
            field,
            calendar_interval,
            min_doc_count,
        } => {
            let f = resolve_field(coll, field)?;
            let interval = crate::query::aggregations::date_histogram::DateInterval::parse_interval(
                calendar_interval,
            );

            if sub_aggs.is_empty() {
                let buckets = compute_date_histogram_buckets(
                    searcher,
                    f,
                    doc_addrs,
                    &interval,
                    *min_doc_count,
                )?;
                AggregationValue::Buckets(buckets)
            } else {
                // Per-bucket doc addresses for sub-aggs
                let mut bucket_docs: std::collections::BTreeMap<String, Vec<tantivy::DocAddress>> =
                    std::collections::BTreeMap::new();
                for &doc_addr in doc_addrs {
                    let doc: TantivyDocument = searcher.doc(doc_addr)?;
                    if let Some(value) = doc.get_first(f) {
                        if let Some(key) = date_value_to_bucket_key(value, &interval) {
                            bucket_docs.entry(key).or_default().push(doc_addr);
                        }
                    }
                }
                let min_count = min_doc_count.unwrap_or(0);
                let mut buckets = Vec::new();
                for (key, addrs) in &bucket_docs {
                    let doc_count = addrs.len() as u64;
                    if doc_count < min_count {
                        continue;
                    }
                    let child_aggs =
                        execute_aggregations(searcher, coll, searchable_fields, addrs, sub_aggs)?;
                    let child_vec: Vec<AggregationResult> = child_aggs.into_values().collect();
                    buckets.push(Bucket {
                        key: key.clone(),
                        doc_count,
                        from: None,
                        to: None,
                        sub_aggs: if child_vec.is_empty() {
                            None
                        } else {
                            Some(child_vec)
                        },
                    });
                }
                AggregationValue::Buckets(buckets)
            }
        }

        AggregationType::Range { field, ranges } => {
            let f = resolve_field(coll, field)?;

            if sub_aggs.is_empty() {
                let (nums, _) = collect_field_values(searcher, f, doc_addrs)?;
                let buckets = compute_range_buckets(&nums, ranges);
                AggregationValue::Buckets(buckets)
            } else {
                // Per-range bucket doc addresses
                let mut range_docs: Vec<(RangeEntry, Vec<tantivy::DocAddress>)> =
                    ranges.iter().map(|r| (r.clone(), Vec::new())).collect();
                for &doc_addr in doc_addrs {
                    let doc: TantivyDocument = searcher.doc(doc_addr)?;
                    if let Some(value) = doc.get_first(f) {
                        let v = match value {
                            tantivy::schema::OwnedValue::U64(n) => Some(*n as f64),
                            tantivy::schema::OwnedValue::I64(n) => Some(*n as f64),
                            tantivy::schema::OwnedValue::F64(n) => Some(*n),
                            _ => None,
                        };
                        if let Some(v) = v {
                            for (range, addrs) in &mut range_docs {
                                let above_from = range.from.is_none_or(|from| v >= from);
                                let below_to = range.to.is_none_or(|to| v < to);
                                if above_from && below_to {
                                    addrs.push(doc_addr);
                                }
                            }
                        }
                    }
                }
                let mut buckets = Vec::new();
                for (range, addrs) in &range_docs {
                    let key = range_bucket_key(range);
                    let child_aggs =
                        execute_aggregations(searcher, coll, searchable_fields, addrs, sub_aggs)?;
                    let child_vec: Vec<AggregationResult> = child_aggs.into_values().collect();
                    buckets.push(Bucket {
                        key,
                        doc_count: addrs.len() as u64,
                        from: range.from,
                        to: range.to,
                        sub_aggs: if child_vec.is_empty() {
                            None
                        } else {
                            Some(child_vec)
                        },
                    });
                }
                AggregationValue::Buckets(buckets)
            }
        }

        AggregationType::Filter { filter } => {
            // Parse the filter query, run on the full index, then intersect with doc_addrs
            let filter_addrs =
                resolve_filter_docs(searcher, coll, searchable_fields, filter, Some(doc_addrs))?;
            let doc_count = filter_addrs.len() as u64;

            if sub_aggs.is_empty() {
                AggregationValue::Buckets(vec![Bucket {
                    key: "filter".to_string(),
                    doc_count,
                    from: None,
                    to: None,
                    sub_aggs: None,
                }])
            } else {
                let child_aggs = execute_aggregations(
                    searcher,
                    coll,
                    searchable_fields,
                    &filter_addrs,
                    sub_aggs,
                )?;
                let child_vec: Vec<AggregationResult> = child_aggs.into_values().collect();
                AggregationValue::Buckets(vec![Bucket {
                    key: "filter".to_string(),
                    doc_count,
                    from: None,
                    to: None,
                    sub_aggs: if child_vec.is_empty() {
                        None
                    } else {
                        Some(child_vec)
                    },
                }])
            }
        }

        AggregationType::Filters { filters } => {
            let mut buckets = Vec::new();
            for (name, filter_query) in filters {
                let filter_addrs = resolve_filter_docs(
                    searcher,
                    coll,
                    searchable_fields,
                    filter_query,
                    Some(doc_addrs),
                )?;
                let doc_count = filter_addrs.len() as u64;

                if sub_aggs.is_empty() {
                    buckets.push(Bucket {
                        key: name.clone(),
                        doc_count,
                        from: None,
                        to: None,
                        sub_aggs: None,
                    });
                } else {
                    let child_aggs = execute_aggregations(
                        searcher,
                        coll,
                        searchable_fields,
                        &filter_addrs,
                        sub_aggs,
                    )?;
                    let child_vec: Vec<AggregationResult> = child_aggs.into_values().collect();
                    buckets.push(Bucket {
                        key: name.clone(),
                        doc_count,
                        from: None,
                        to: None,
                        sub_aggs: if child_vec.is_empty() {
                            None
                        } else {
                            Some(child_vec)
                        },
                    });
                }
            }
            AggregationValue::Buckets(buckets)
        }

        AggregationType::Global {} => {
            // Run on ALL documents, ignoring the query filter
            let all_addrs = resolve_filter_docs(searcher, coll, searchable_fields, "*", None)?;
            let doc_count = all_addrs.len() as u64;

            if sub_aggs.is_empty() {
                AggregationValue::Buckets(vec![Bucket {
                    key: "global".to_string(),
                    doc_count,
                    from: None,
                    to: None,
                    sub_aggs: None,
                }])
            } else {
                let child_aggs =
                    execute_aggregations(searcher, coll, searchable_fields, &all_addrs, sub_aggs)?;
                let child_vec: Vec<AggregationResult> = child_aggs.into_values().collect();
                AggregationValue::Buckets(vec![Bucket {
                    key: "global".to_string(),
                    doc_count,
                    from: None,
                    to: None,
                    sub_aggs: if child_vec.is_empty() {
                        None
                    } else {
                        Some(child_vec)
                    },
                }])
            }
        }
    };

    Ok(AggregationResult {
        name: agg_req.name.clone(),
        value,
    })
}

/// Resolve a field name to a tantivy Field
fn resolve_field(coll: &CollectionIndex, field_name: &str) -> Result<Field> {
    coll.field_map
        .get(field_name)
        .copied()
        .ok_or_else(|| Error::InvalidQuery(format!("Unknown field: {}", field_name)))
}

/// Compute histogram buckets from numeric values
fn compute_histogram_buckets(
    values: &[f64],
    interval: f64,
    min_doc_count: Option<u64>,
    extended_bounds: Option<&HistogramBounds>,
) -> Vec<Bucket> {
    let mut counts: std::collections::BTreeMap<i64, u64> = std::collections::BTreeMap::new();

    for &v in values {
        let bucket_key = (v / interval).floor() as i64;
        *counts.entry(bucket_key).or_insert(0) += 1;
    }

    // Apply extended bounds: ensure all buckets within range exist
    if let Some(bounds) = extended_bounds {
        let min_key = (bounds.min / interval).floor() as i64;
        let max_key = (bounds.max / interval).floor() as i64;
        for k in min_key..=max_key {
            counts.entry(k).or_insert(0);
        }
    }

    let min_count = min_doc_count.unwrap_or(0);

    counts
        .into_iter()
        .filter(|(_, count)| *count >= min_count)
        .map(|(k, count)| Bucket {
            key: format!("{}", (k as f64) * interval),
            doc_count: count,
            from: None,
            to: None,
            sub_aggs: None,
        })
        .collect()
}

/// Compute date histogram buckets from document field values
fn compute_date_histogram_buckets(
    searcher: &tantivy::Searcher,
    field: Field,
    doc_addrs: &[tantivy::DocAddress],
    interval: &Option<crate::query::aggregations::date_histogram::DateInterval>,
    min_doc_count: Option<u64>,
) -> Result<Vec<Bucket>> {
    let mut counts: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();

    for &doc_addr in doc_addrs {
        let doc: TantivyDocument = searcher.doc(doc_addr)?;
        if let Some(value) = doc.get_first(field) {
            if let Some(key) = date_value_to_bucket_key(value, interval) {
                *counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    let min_count = min_doc_count.unwrap_or(0);

    Ok(counts
        .into_iter()
        .filter(|(_, count)| *count >= min_count)
        .map(|(key, count)| Bucket {
            key,
            doc_count: count,
            from: None,
            to: None,
            sub_aggs: None,
        })
        .collect())
}

/// Convert a tantivy field value to a date histogram bucket key
fn date_value_to_bucket_key(
    value: &tantivy::schema::OwnedValue,
    interval: &Option<crate::query::aggregations::date_histogram::DateInterval>,
) -> Option<String> {
    // Try to extract a timestamp (stored as i64 micros or u64 micros or Date)
    let micros = match value {
        tantivy::schema::OwnedValue::I64(n) => Some(*n),
        tantivy::schema::OwnedValue::U64(n) => Some(*n as i64),
        tantivy::schema::OwnedValue::Date(dt) => Some(dt.into_timestamp_micros()),
        _ => None,
    };

    if let Some(micros) = micros {
        let secs = micros / 1_000_000;
        let dt = chrono::DateTime::from_timestamp(secs, 0)?;
        let dt_utc: chrono::DateTime<chrono::Utc> = dt;

        if let Some(interval) = interval {
            Some(interval.floor(dt_utc).to_rfc3339())
        } else {
            // Default to day if no interval parsed
            Some(
                crate::query::aggregations::date_histogram::DateInterval::Day
                    .floor(dt_utc)
                    .to_rfc3339(),
            )
        }
    } else {
        None
    }
}

/// Compute range buckets from numeric values
fn compute_range_buckets(values: &[f64], ranges: &[RangeEntry]) -> Vec<Bucket> {
    ranges
        .iter()
        .map(|range| {
            let count = values
                .iter()
                .filter(|&&v| {
                    let above_from = range.from.is_none_or(|from| v >= from);
                    let below_to = range.to.is_none_or(|to| v < to);
                    above_from && below_to
                })
                .count() as u64;

            Bucket {
                key: range_bucket_key(range),
                doc_count: count,
                from: range.from,
                to: range.to,
                sub_aggs: None,
            }
        })
        .collect()
}

/// Generate a bucket key for a range entry
fn range_bucket_key(range: &RangeEntry) -> String {
    if let Some(ref key) = range.key {
        return key.clone();
    }
    match (range.from, range.to) {
        (Some(from), Some(to)) => format!("{}-{}", from, to),
        (Some(from), None) => format!("{}-*", from),
        (None, Some(to)) => format!("*-{}", to),
        (None, None) => "*-*".to_string(),
    }
}

/// Run a filter query and return matching doc addresses, optionally intersected with a parent set
fn resolve_filter_docs(
    searcher: &tantivy::Searcher,
    coll: &CollectionIndex,
    searchable_fields: &[Field],
    query_str: &str,
    parent_addrs: Option<&[tantivy::DocAddress]>,
) -> Result<Vec<tantivy::DocAddress>> {
    let qp = QueryParser::for_index(&coll.index, searchable_fields.to_vec());
    let qs = query_str.to_string();
    let parsed = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        qp.parse_query(&qs)
    })) {
        Ok(Ok(q)) => q,
        Ok(Err(e)) => return Err(Error::InvalidQuery(e.to_string())),
        Err(_) => {
            return Err(Error::InvalidQuery(format!(
                "Query parser panicked on input: {:?}",
                qs
            )));
        }
    };

    let results = searcher.search(&parsed, &TopDocs::with_limit(10000))?;
    let addrs: Vec<tantivy::DocAddress> = results.into_iter().map(|(_s, addr)| addr).collect();

    if let Some(parent) = parent_addrs {
        // Intersect: only keep addresses that are in the parent set
        let parent_set: std::collections::HashSet<_> = parent.iter().collect();
        Ok(addrs
            .into_iter()
            .filter(|a| parent_set.contains(a))
            .collect())
    } else {
        Ok(addrs)
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

/// Suggestion entry returned by suggest_terms
#[derive(Debug, Clone, serde::Serialize)]
pub struct SuggestEntry {
    pub term: String,
    pub score: f32,
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
    pub fn get_top_terms(
        &self,
        collection: &str,
        field: &str,
        limit: usize,
    ) -> Result<Vec<TermInfo>> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let field_obj = coll
            .field_map
            .get(field)
            .ok_or_else(|| Error::Schema(format!("Field '{}' not found", field)))?;

        coll.reader.reload()?;
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

    /// Suggest terms from the index using prefix matching and optional fuzzy correction.
    pub fn suggest_terms(
        &self,
        collection: &str,
        field: &str,
        prefix: &str,
        size: usize,
        fuzzy: bool,
        max_distance: usize,
    ) -> Result<Vec<SuggestEntry>> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        let field_obj = coll
            .field_map
            .get(field)
            .ok_or_else(|| Error::Schema(format!("Field '{}' not found", field)))?;

        coll.reader.reload()?;
        let searcher = coll.reader.searcher();
        let prefix_bytes = prefix.as_bytes();
        let mut term_counts: HashMap<String, u64> = HashMap::new();

        // Prefix-filtered term stream across all segments
        for segment_reader in searcher.segment_readers() {
            let inverted_index = segment_reader.inverted_index(*field_obj)?;
            let term_dict = inverted_index.terms();
            let mut term_stream = term_dict.range().ge(prefix_bytes).into_stream()?;

            while term_stream.advance() {
                let term_bytes = term_stream.key();
                // Stop once we pass the prefix range
                if !term_bytes.starts_with(prefix_bytes) {
                    break;
                }
                if let Ok(term_str) = std::str::from_utf8(term_bytes) {
                    let doc_freq = term_stream.value().doc_freq as u64;
                    *term_counts.entry(term_str.to_string()).or_insert(0) += doc_freq;
                }
            }
        }

        // Find max doc_freq for normalization
        let max_df = term_counts.values().copied().max().unwrap_or(1) as f32;

        let mut entries: Vec<SuggestEntry> = term_counts
            .into_iter()
            .map(|(term, doc_freq)| {
                // Prefix matches get score=1.0 weighted by doc_freq
                let score = doc_freq as f32 / max_df;
                SuggestEntry {
                    term,
                    score,
                    doc_freq,
                }
            })
            .collect();

        // If fuzzy is enabled and we have few prefix results, add fuzzy matches
        if fuzzy && entries.len() < size {
            // Collect all terms from the field for fuzzy matching
            let mut all_terms: Vec<String> = Vec::new();
            for segment_reader in searcher.segment_readers() {
                let inverted_index = segment_reader.inverted_index(*field_obj)?;
                let term_dict = inverted_index.terms();
                let mut term_stream = term_dict.stream()?;
                while term_stream.advance() {
                    if let Ok(t) = std::str::from_utf8(term_stream.key()) {
                        all_terms.push(t.to_string());
                    }
                }
            }
            all_terms.sort();
            all_terms.dedup();

            let fuzzy_suggestions = crate::query::suggestions::suggest_corrections(
                prefix,
                &all_terms,
                max_distance,
                size,
            );

            let existing_terms: std::collections::HashSet<String> =
                entries.iter().map(|e| e.term.clone()).collect();

            for suggestion in fuzzy_suggestions {
                if existing_terms.contains(&suggestion.term) {
                    continue;
                }
                // Look up doc_freq for the fuzzy match
                let mut doc_freq: u64 = 0;
                for segment_reader in searcher.segment_readers() {
                    let inverted_index = segment_reader.inverted_index(*field_obj)?;
                    let term = Term::from_field_text(*field_obj, &suggestion.term);
                    doc_freq += inverted_index.doc_freq(&term)? as u64;
                }
                entries.push(SuggestEntry {
                    term: suggestion.term,
                    score: suggestion.score * 0.8, // Discount fuzzy matches
                    doc_freq,
                });
            }
        }

        // Sort by score descending, then by doc_freq descending
        entries.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap()
                .then_with(|| b.doc_freq.cmp(&a.doc_freq))
        });
        entries.truncate(size);

        Ok(entries)
    }

    /// More Like This: find documents similar to a given document or text.
    ///
    /// Extracts significant terms from the source (by ID or raw text), builds
    /// a disjunction query, and returns the top matching documents (excluding
    /// the source document when searching by ID).
    #[allow(clippy::too_many_arguments)]
    pub fn more_like_this(
        &self,
        collection: &str,
        doc_id: Option<&str>,
        like_text: Option<&str>,
        fields: &[String],
        min_term_freq: usize,
        min_doc_freq: u64,
        max_query_terms: usize,
        size: usize,
    ) -> Result<SearchResults> {
        let start = std::time::Instant::now();
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        coll.reader.reload()?;
        let searcher = coll.reader.searcher();

        // Determine source text: either from an existing document or from provided text
        let source_text: String;
        let exclude_id: Option<String>;

        if let Some(id) = doc_id {
            exclude_id = Some(id.to_string());
            // Fetch the document
            let id_field = coll.field_map.get("id").unwrap();
            let term = Term::from_field_text(*id_field, id);
            let query = tantivy::query::TermQuery::new(term, IndexRecordOption::Basic);
            let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;

            if let Some((_score, doc_addr)) = top_docs.first() {
                let doc: TantivyDocument = searcher.doc(*doc_addr)?;
                let mut texts = Vec::new();
                for field_name in fields {
                    if let Some(&field) = coll.field_map.get(field_name) {
                        if let Some(val) = doc.get_first(field) {
                            if let Some(s) = val.as_str() {
                                texts.push(s.to_string());
                            }
                        }
                    }
                }
                source_text = texts.join(" ");
            } else {
                return Ok(SearchResults {
                    results: vec![],
                    total: 0,
                    latency_ms: 0,
                });
            }
        } else if let Some(text) = like_text {
            exclude_id = None;
            source_text = text.to_string();
        } else {
            return Err(Error::InvalidQuery(
                "Either 'like._id' or 'like_text' must be provided".to_string(),
            ));
        }

        if source_text.is_empty() {
            return Ok(SearchResults {
                results: vec![],
                total: 0,
                latency_ms: 0,
            });
        }

        // Extract significant terms: tokenize the source text and count term frequencies
        let mut term_freqs: HashMap<String, usize> = HashMap::new();
        for word in source_text.split_whitespace() {
            let w = word.to_lowercase();
            // Skip very short tokens
            if w.len() >= 2 {
                *term_freqs.entry(w).or_insert(0) += 1;
            }
        }

        // Filter by min_term_freq and min_doc_freq, then score by TF-IDF-like weighting
        let resolve_fields: Vec<Field> = if fields.is_empty() {
            coll.schema
                .fields()
                .filter(|(_, e)| {
                    matches!(e.field_type(), tantivy::schema::FieldType::Str(_))
                        && e.field_type().is_indexed()
                })
                .map(|(f, _)| f)
                .collect()
        } else {
            fields
                .iter()
                .filter_map(|f| coll.field_map.get(f).copied())
                .collect()
        };

        let num_docs = searcher.num_docs() as f32;
        let mut scored_terms: Vec<(String, f32)> = Vec::new();

        for (term_str, tf) in &term_freqs {
            if *tf < min_term_freq {
                continue;
            }
            // Sum doc_freq across all target fields
            let mut total_df: u64 = 0;
            for &field in &resolve_fields {
                let t = Term::from_field_text(field, term_str);
                total_df += searcher.doc_freq(&t)?;
            }
            if total_df < min_doc_freq {
                continue;
            }
            // TF-IDF score
            let idf = (num_docs / (1.0 + total_df as f32)).ln() + 1.0;
            let score = (*tf as f32) * idf;
            scored_terms.push((term_str.clone(), score));
        }

        // Sort by score desc and take top max_query_terms
        scored_terms.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored_terms.truncate(max_query_terms);

        if scored_terms.is_empty() {
            return Ok(SearchResults {
                results: vec![],
                total: 0,
                latency_ms: 0,
            });
        }

        // Build a disjunction query from the top terms
        let query_string = scored_terms
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        let query_parser = QueryParser::for_index(&coll.index, resolve_fields);
        let qs = query_string.clone();
        let parsed_query = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            query_parser.parse_query(&qs)
        })) {
            Ok(Ok(q)) => q,
            Ok(Err(e)) => return Err(Error::InvalidQuery(e.to_string())),
            Err(_) => {
                return Err(Error::InvalidQuery(format!(
                    "Query parser panicked on input: {:?}",
                    qs
                )));
            }
        };

        // Search for size + 1 to allow excluding the source doc
        let fetch_limit = if exclude_id.is_some() { size + 1 } else { size };
        let top_docs = searcher.search(&parsed_query, &TopDocs::with_limit(fetch_limit))?;

        let id_field = coll.field_map.get("id").unwrap();
        let mut results = Vec::new();

        for (score, doc_addr) in &top_docs {
            let doc: TantivyDocument = searcher.doc(*doc_addr)?;
            let id = doc
                .get_first(*id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Skip the source document
            if let Some(ref eid) = exclude_id {
                if id == *eid {
                    continue;
                }
            }

            let mut fields_map = HashMap::new();
            for (field, entry) in coll.schema.fields() {
                if entry.is_stored() {
                    if let Some(value) = doc.get_first(field) {
                        if let Some(json_value) = owned_value_to_json(value) {
                            fields_map.insert(entry.name().to_string(), json_value);
                        }
                    }
                }
            }

            results.push(SearchResult {
                id,
                score: *score,
                fields: fields_map,
                highlight: None,
            });

            if results.len() >= size {
                break;
            }
        }

        let total = results.len();
        let latency_ms = start.elapsed().as_millis() as u64;
        Ok(SearchResults {
            results,
            total,
            latency_ms,
        })
    }

    /// Get segment information for a collection.
    pub fn get_segments(&self, collection: &str) -> Result<SegmentsInfo> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        coll.reader.reload()?;
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
    pub fn reconstruct_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<ReconstructedDocument>> {
        let collections = self.collections.read().unwrap();
        let coll = collections
            .get(collection)
            .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

        coll.reader.reload()?;
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
                    if let Some(json_value) = owned_value_to_json(value) {
                        stored_fields.insert(entry.name().to_string(), json_value);
                    }
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
                                term_stream.value(),
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
