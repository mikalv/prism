//! Live detach/attach operations for collections.
//!
//! Detach: snapshot-export a collection, unload from the running server, optionally delete on-disk data.
//! Attach: import a snapshot, load schema, hot-add the collection to the running server.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::collection::CollectionManager;
use crate::error::{Error, Result};
use crate::export::snapshot::{export_snapshot, import_snapshot, SnapshotImportResult};
use crate::export::types::{ExportMetadata, ExportProgress};
use crate::schema::CollectionSchema;

/// Where a detached snapshot is written.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DetachDestination {
    File { path: PathBuf },
}

/// Result returned after a successful detach.
#[derive(Debug, Serialize)]
pub struct DetachResult {
    pub collection: String,
    pub destination: DetachDestination,
    pub metadata: ExportMetadata,
    pub data_deleted: bool,
}

/// Source to attach a collection from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AttachSource {
    File { path: PathBuf },
}

/// Result returned after a successful attach.
#[derive(Debug, Serialize)]
pub struct AttachResult {
    pub collection: String,
    pub source: AttachSource,
    pub files_extracted: u64,
    pub bytes_extracted: u64,
}

/// Detach a collection from a running server.
///
/// Safe order: export snapshot first, verify it, then unload and optionally delete data.
///
/// # Arguments
/// * `manager`     - The running CollectionManager
/// * `data_dir`    - Base data directory (contains `collections/`)
/// * `collection`  - Name of the collection to detach
/// * `destination` - Where to write the snapshot
/// * `delete_data` - Whether to remove on-disk data after unloading
/// * `progress`    - Optional progress callback
pub async fn detach_collection(
    manager: &Arc<CollectionManager>,
    data_dir: &Path,
    collection: &str,
    destination: &DetachDestination,
    delete_data: bool,
    progress: Option<&dyn ExportProgress>,
) -> Result<DetachResult> {
    // 1. Verify the collection exists
    if manager.get_schema(collection).is_none() {
        return Err(Error::CollectionNotFound(collection.to_string()));
    }

    // 2. Export snapshot (safe — never delete before this succeeds)
    let output_path = match destination {
        DetachDestination::File { path } => path.clone(),
    };
    let metadata = export_snapshot(data_dir, collection, &output_path, progress)?;

    // 3. Verify export produced a non-empty file
    let file_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);
    if file_size == 0 {
        return Err(Error::Export(
            "Snapshot export produced an empty file, aborting detach".to_string(),
        ));
    }

    // 4. Unload from the running server
    manager.remove_collection(collection).await?;

    // 5. Optionally delete on-disk data
    let data_deleted = if delete_data {
        let collection_dir = data_dir.join("collections").join(collection);
        if collection_dir.exists() {
            std::fs::remove_dir_all(&collection_dir).map_err(|e| {
                Error::Export(format!(
                    "Collection unloaded but failed to delete data directory: {}",
                    e
                ))
            })?;
        }
        true
    } else {
        false
    };

    Ok(DetachResult {
        collection: collection.to_string(),
        destination: destination.clone(),
        metadata,
        data_deleted,
    })
}

/// Attach (re-load) a collection from a snapshot into a running server.
///
/// # Arguments
/// * `manager`           - The running CollectionManager
/// * `data_dir`          - Base data directory (contains `collections/`)
/// * `source`            - Where to read the snapshot from
/// * `target_collection` - Optional override name (defaults to name in snapshot)
/// * `progress`          - Optional progress callback
pub async fn attach_collection(
    manager: &Arc<CollectionManager>,
    data_dir: &Path,
    source: &AttachSource,
    target_collection: Option<&str>,
    progress: Option<&dyn ExportProgress>,
) -> Result<AttachResult> {
    // 1. Import snapshot to disk
    let input_path = match source {
        AttachSource::File { path } => path.clone(),
    };
    let import_result: SnapshotImportResult =
        import_snapshot(data_dir, &input_path, target_collection, progress)?;

    let collection_name = &import_result.collection;

    // 2. Check the collection isn't already loaded
    if manager.get_schema(collection_name).is_some() {
        return Err(Error::Import(format!(
            "Collection '{}' already exists in the running server",
            collection_name
        )));
    }

    // 3. Load schema from extracted data
    let schema_path = data_dir
        .join("collections")
        .join(collection_name)
        .join("schema.yaml");
    if !schema_path.exists() {
        return Err(Error::Import(
            "Snapshot does not contain schema.yaml — cannot attach".to_string(),
        ));
    }
    let schema_content = std::fs::read_to_string(&schema_path)
        .map_err(|e| Error::Import(format!("Failed to read schema.yaml: {}", e)))?;
    let mut schema: CollectionSchema = serde_yaml::from_str(&schema_content)
        .map_err(|e| Error::Import(format!("Failed to parse schema.yaml: {}", e)))?;

    // If we renamed the collection, update the schema to match
    if let Some(target) = target_collection {
        schema.collection = target.to_string();
    }

    // 4. Hot-load into running server
    manager.add_collection(schema).await?;

    Ok(AttachResult {
        collection: collection_name.clone(),
        source: source.clone(),
        files_extracted: import_result.files_extracted,
        bytes_extracted: import_result.bytes_extracted,
    })
}
