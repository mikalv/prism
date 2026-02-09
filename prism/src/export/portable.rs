//! Portable (JSON-based) export/import implementation.
//!
//! Format:
//! - Line 1: JSON metadata object
//! - Line 2: YAML schema (base64 encoded in JSON)
//! - Lines 3+: NDJSON documents

use crate::backends::{Document, SearchBackend, TextBackend, VectorBackend};
use crate::collection::CollectionManager;
use crate::error::{Error, Result};
use crate::schema::CollectionSchema;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, Write};
use std::sync::Arc;

use super::types::{ExportBackendInfo, ExportMetadata, ExportProgress, ExportedDocument};

/// Portable export header (first line of file).
#[derive(Debug, Serialize, Deserialize)]
struct PortableHeader {
    pub format: String,
    pub metadata: ExportMetadata,
    /// Base64-encoded YAML schema
    pub schema_b64: String,
}

/// Export a collection in portable JSON format.
///
/// # Arguments
/// * `manager` - Collection manager with access to backends
/// * `collection` - Name of the collection to export
/// * `writer` - Output destination
/// * `progress` - Progress callback (optional)
pub async fn export_portable<W: Write>(
    manager: &CollectionManager,
    collection: &str,
    mut writer: W,
    progress: Option<&dyn ExportProgress>,
) -> Result<ExportMetadata> {
    // Get schema
    let schema = manager
        .get_schema(collection)
        .ok_or_else(|| Error::CollectionNotFound(collection.to_string()))?;

    // Get stats for document count
    let stats = manager.stats(collection).await?;
    let total_docs = stats.document_count as u64;

    // Serialize schema to YAML
    let schema_yaml = serde_yaml::to_string(&schema)
        .map_err(|e| Error::Export(format!("Schema serialization failed: {}", e)))?;
    let schema_b64 = BASE64.encode(schema_yaml.as_bytes());

    // Build metadata
    let metadata = ExportMetadata {
        version: "1.0".to_string(),
        collection: collection.to_string(),
        prism_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        document_count: total_docs,
        size_bytes: stats.size_bytes as u64,
        checksum: None, // Will be computed after export
        backends: ExportBackendInfo {
            text: schema.backends.text.is_some(),
            vector: schema.backends.vector.is_some(),
            graph: schema.backends.graph.is_some(),
        },
    };

    // Write header
    let header = PortableHeader {
        format: "prism-portable-v1".to_string(),
        metadata: metadata.clone(),
        schema_b64,
    };
    let header_json = serde_json::to_string(&header)
        .map_err(|e| Error::Export(format!("Header serialization failed: {}", e)))?;
    writeln!(writer, "{}", header_json)
        .map_err(|e| Error::Export(format!("Write failed: {}", e)))?;

    // Export documents using scroll-like iteration
    let mut hasher = Sha256::new();
    let mut exported_count = 0u64;

    // Iterate all documents using text backend's searcher
    let docs = iterate_all_documents(manager, collection, &schema).await?;

    for doc in docs {
        let doc_json = serde_json::to_string(&doc)
            .map_err(|e| Error::Export(format!("Document serialization failed: {}", e)))?;

        hasher.update(doc_json.as_bytes());
        writeln!(writer, "{}", doc_json)
            .map_err(|e| Error::Export(format!("Write failed: {}", e)))?;

        exported_count += 1;
        if let Some(p) = progress {
            if exported_count.is_multiple_of(1000) || exported_count == total_docs {
                p.on_progress(exported_count, total_docs, "Exporting documents");
            }
        }
    }

    writer
        .flush()
        .map_err(|e| Error::Export(format!("Flush failed: {}", e)))?;

    if let Some(p) = progress {
        p.on_complete(exported_count);
    }

    // Return metadata with checksum
    let checksum = hex::encode(hasher.finalize());
    Ok(ExportMetadata {
        checksum: Some(checksum),
        document_count: exported_count,
        ..metadata
    })
}

/// Iterate all documents in a collection.
async fn iterate_all_documents(
    manager: &CollectionManager,
    collection: &str,
    schema: &CollectionSchema,
) -> Result<Vec<ExportedDocument>> {
    let mut documents = Vec::new();

    // Use a match-all query with high limit to get all documents
    // This is a simplification - for very large collections, we'd need pagination
    let stats = manager.stats(collection).await?;
    let total = stats.document_count;

    if total == 0 {
        return Ok(documents);
    }

    // Search with match-all query (empty query string matches all in Tantivy)
    let query = crate::backends::Query {
        query_string: "*".to_string(),
        fields: vec![],
        limit: total.max(10000), // Tantivy's TopDocs limit
        offset: 0,
        merge_strategy: None,
        text_weight: None,
        vector_weight: None,
        highlight: None,
    };

    let results = manager.search(collection, query, None).await?;

    // Get vectors if vector backend is enabled
    let has_vector = schema.backends.vector.is_some();

    for result in results.results {
        let mut exported = ExportedDocument {
            id: result.id.clone(),
            fields: result.fields,
            vector: None,
        };

        // If vector backend is enabled, try to get the vector
        if has_vector {
            // The vector should be stored in the document fields if configured
            if let Some(vector_config) = &schema.backends.vector {
                if let Some(vec_value) = exported.fields.get(&vector_config.embedding_field) {
                    if let Some(vec_arr) = vec_value.as_array() {
                        let vec: Vec<f32> = vec_arr
                            .iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect();
                        if !vec.is_empty() {
                            exported.vector = Some(vec);
                        }
                    }
                }
            }
        }

        documents.push(exported);
    }

    Ok(documents)
}

/// Import a collection from portable JSON format.
///
/// # Arguments
/// * `text_backend` - Text backend for indexing
/// * `vector_backend` - Vector backend for embeddings
/// * `reader` - Input source
/// * `target_collection` - Name for the imported collection (overrides source name if provided)
/// * `progress` - Progress callback (optional)
pub async fn import_portable<R: BufRead>(
    text_backend: Arc<TextBackend>,
    vector_backend: Arc<VectorBackend>,
    reader: R,
    target_collection: Option<&str>,
    progress: Option<&dyn ExportProgress>,
) -> Result<ImportResult> {
    let mut lines = reader.lines();

    // Read header
    let header_line = lines
        .next()
        .ok_or_else(|| Error::Import("Empty file".to_string()))?
        .map_err(|e| Error::Import(format!("Read error: {}", e)))?;

    let header: PortableHeader = serde_json::from_str(&header_line)
        .map_err(|e| Error::Import(format!("Invalid header: {}", e)))?;

    if !header.format.starts_with("prism-portable-v") {
        return Err(Error::Import(format!(
            "Unsupported format: {}",
            header.format
        )));
    }

    // Decode schema
    let schema_yaml = BASE64
        .decode(&header.schema_b64)
        .map_err(|e| Error::Import(format!("Schema decode failed: {}", e)))?;
    let schema_str = String::from_utf8(schema_yaml)
        .map_err(|e| Error::Import(format!("Schema UTF-8 error: {}", e)))?;
    let mut schema: CollectionSchema = serde_yaml::from_str(&schema_str)
        .map_err(|e| Error::Import(format!("Schema parse failed: {}", e)))?;

    // Override collection name if specified
    let collection_name = target_collection.unwrap_or(&header.metadata.collection);
    schema.collection = collection_name.to_string();

    // Initialize backends with the schema
    if schema.backends.text.is_some() {
        text_backend.initialize(collection_name, &schema).await?;
    }
    if schema.backends.vector.is_some() {
        vector_backend.initialize(collection_name, &schema).await?;
    }

    // Import documents
    let mut imported_count = 0u64;
    let mut batch: Vec<Document> = Vec::new();
    let batch_size = 1000;
    let total = header.metadata.document_count;

    let mut hasher = Sha256::new();

    for line_result in lines {
        let line = line_result.map_err(|e| Error::Import(format!("Read error: {}", e)))?;
        if line.trim().is_empty() {
            continue;
        }

        hasher.update(line.as_bytes());

        let exported: ExportedDocument = serde_json::from_str(&line)
            .map_err(|e| Error::Import(format!("Document parse failed: {}", e)))?;

        // Convert to Document
        let mut fields = exported.fields;

        // Add vector back to fields if present
        if let Some(vec) = exported.vector {
            if let Some(vector_config) = &schema.backends.vector {
                fields.insert(
                    vector_config.embedding_field.clone(),
                    serde_json::Value::Array(vec.iter().map(|f| serde_json::json!(*f)).collect()),
                );
            }
        }

        batch.push(Document {
            id: exported.id,
            fields,
        });

        if batch.len() >= batch_size {
            // Index batch
            if schema.backends.text.is_some() {
                text_backend.index(collection_name, batch.clone()).await?;
            }
            if schema.backends.vector.is_some() {
                vector_backend.index(collection_name, batch.clone()).await?;
            }

            imported_count += batch.len() as u64;
            batch.clear();

            if let Some(p) = progress {
                p.on_progress(imported_count, total, "Importing documents");
            }
        }
    }

    // Index remaining documents
    if !batch.is_empty() {
        if schema.backends.text.is_some() {
            text_backend.index(collection_name, batch.clone()).await?;
        }
        if schema.backends.vector.is_some() {
            vector_backend.index(collection_name, batch.clone()).await?;
        }
        imported_count += batch.len() as u64;
    }

    if let Some(p) = progress {
        p.on_complete(imported_count);
    }

    // Verify checksum if provided
    let computed_checksum = hex::encode(hasher.finalize());
    let checksum_valid = header
        .metadata
        .checksum
        .as_ref()
        .map(|expected| expected == &computed_checksum)
        .unwrap_or(true);

    if !checksum_valid {
        return Err(Error::Import("Checksum verification failed".to_string()));
    }

    Ok(ImportResult {
        collection: collection_name.to_string(),
        documents_imported: imported_count,
        checksum_valid,
    })
}

/// Result of an import operation.
#[derive(Debug)]
pub struct ImportResult {
    pub collection: String,
    pub documents_imported: u64,
    pub checksum_valid: bool,
}
