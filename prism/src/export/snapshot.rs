//! Snapshot (binary) export/import implementation.
//!
//! Creates a compressed tar archive containing:
//! - metadata.json: Export metadata
//! - schema.yaml: Collection schema
//! - text/: Tantivy index files
//! - vector/: Vector index files
//! - graph/: Graph backend files (if present)

use crate::error::{Error, Result};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::Path;

use super::types::{ExportBackendInfo, ExportMetadata, ExportProgress};

/// Export a collection as a compressed binary snapshot.
///
/// Creates a tar.zst archive with all backend data files.
///
/// # Arguments
/// * `data_dir` - Base data directory containing collection data
/// * `collection` - Name of the collection to export
/// * `output_path` - Output file path for the archive
/// * `progress` - Progress callback (optional)
pub fn export_snapshot(
    data_dir: &Path,
    collection: &str,
    output_path: &Path,
    progress: Option<&dyn ExportProgress>,
) -> Result<ExportMetadata> {
    let collection_dir = data_dir.join("collections").join(collection);

    if !collection_dir.exists() {
        return Err(Error::CollectionNotFound(collection.to_string()));
    }

    // Calculate total size for progress
    let total_size = calculate_dir_size(&collection_dir)?;
    let mut processed_size = 0u64;

    // Create output file
    let output_file =
        File::create(output_path).map_err(|e| Error::Export(format!("Cannot create output file: {}", e)))?;

    // Create zstd encoder
    let encoder = zstd::stream::Encoder::new(output_file, 3)
        .map_err(|e| Error::Export(format!("Zstd encoder creation failed: {}", e)))?;

    // Create tar archive
    let mut archive = tar::Builder::new(encoder);

    // Add metadata.json
    let metadata = ExportMetadata {
        version: "1.0".to_string(),
        collection: collection.to_string(),
        prism_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        document_count: 0, // Not tracked in snapshot format
        size_bytes: total_size,
        checksum: None,
        backends: ExportBackendInfo {
            text: collection_dir.join("text").exists(),
            vector: collection_dir.join("vector").exists(),
            graph: collection_dir.join("graph").exists(),
        },
    };

    let metadata_json = serde_json::to_string_pretty(&metadata)
        .map_err(|e| Error::Export(format!("Metadata serialization failed: {}", e)))?;
    add_bytes_to_archive(&mut archive, "metadata.json", metadata_json.as_bytes())?;

    // Add schema.yaml if it exists
    let schema_path = collection_dir.join("schema.yaml");
    if schema_path.exists() {
        add_file_to_archive(&mut archive, &schema_path, "schema.yaml", &mut processed_size, total_size, progress)?;
    }

    // Add text backend files
    let text_dir = collection_dir.join("text");
    if text_dir.exists() {
        add_directory_to_archive(&mut archive, &text_dir, "text", &mut processed_size, total_size, progress)?;
    }

    // Add vector backend files
    let vector_dir = collection_dir.join("vector");
    if vector_dir.exists() {
        add_directory_to_archive(&mut archive, &vector_dir, "vector", &mut processed_size, total_size, progress)?;
    }

    // Add graph backend files
    let graph_dir = collection_dir.join("graph");
    if graph_dir.exists() {
        add_directory_to_archive(&mut archive, &graph_dir, "graph", &mut processed_size, total_size, progress)?;
    }

    // Finish archive
    let encoder = archive
        .into_inner()
        .map_err(|e| Error::Export(format!("Archive finalization failed: {}", e)))?;
    encoder
        .finish()
        .map_err(|e| Error::Export(format!("Zstd finalization failed: {}", e)))?;

    if let Some(p) = progress {
        p.on_complete(processed_size);
    }

    Ok(ExportMetadata {
        document_count: 0, // Document count not tracked in snapshot format
        size_bytes: processed_size,
        ..metadata
    })
}

/// Import a collection from a compressed binary snapshot.
///
/// Extracts a tar.zst archive to the data directory.
///
/// # Arguments
/// * `data_dir` - Base data directory for extraction
/// * `input_path` - Input archive file path
/// * `target_collection` - Optional name for the imported collection
/// * `progress` - Progress callback (optional)
pub fn import_snapshot(
    data_dir: &Path,
    input_path: &Path,
    target_collection: Option<&str>,
    progress: Option<&dyn ExportProgress>,
) -> Result<SnapshotImportResult> {
    // Open input file
    let input_file =
        File::open(input_path).map_err(|e| Error::Import(format!("Cannot open input file: {}", e)))?;
    let total_size = input_file
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    // Create zstd decoder
    let decoder = zstd::stream::Decoder::new(BufReader::new(input_file))
        .map_err(|e| Error::Import(format!("Zstd decoder creation failed: {}", e)))?;

    // Open tar archive
    let mut archive = tar::Archive::new(decoder);
    let mut metadata: Option<ExportMetadata> = None;

    // Read metadata first
    for entry_result in archive
        .entries()
        .map_err(|e| Error::Import(format!("Cannot read entries: {}", e)))?
    {
        let mut entry = entry_result.map_err(|e| Error::Import(format!("Entry read failed: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| Error::Import(format!("Path read failed: {}", e)))?
            .to_path_buf();

        if path.to_string_lossy() == "metadata.json" {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| Error::Import(format!("Metadata read failed: {}", e)))?;
            metadata = Some(
                serde_json::from_str(&content)
                    .map_err(|e| Error::Import(format!("Metadata parse failed: {}", e)))?,
            );
            break;
        }
    }

    let meta = metadata.ok_or_else(|| Error::Import("No metadata.json in archive".to_string()))?;
    let collection_name = target_collection
        .map(|s| s.to_string())
        .unwrap_or(meta.collection.clone());

    // Create collection directory
    let collection_dir = data_dir.join("collections").join(&collection_name);
    fs::create_dir_all(&collection_dir)
        .map_err(|e| Error::Import(format!("Cannot create collection directory: {}", e)))?;

    // Re-open archive for extraction
    let input_file =
        File::open(input_path).map_err(|e| Error::Import(format!("Cannot open input file: {}", e)))?;
    let decoder = zstd::stream::Decoder::new(BufReader::new(input_file))
        .map_err(|e| Error::Import(format!("Zstd decoder creation failed: {}", e)))?;
    let mut archive = tar::Archive::new(decoder);

    let mut extracted_size = 0u64;
    let mut file_count = 0u64;

    // Extract all files
    for entry_result in archive
        .entries()
        .map_err(|e| Error::Import(format!("Cannot read entries: {}", e)))?
    {
        let mut entry = entry_result.map_err(|e| Error::Import(format!("Entry read failed: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| Error::Import(format!("Path read failed: {}", e)))?
            .to_path_buf();

        let dest_path = collection_dir.join(&path);

        // Create parent directories
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::Import(format!("Cannot create directory: {}", e)))?;
        }

        // Extract file
        if entry.header().entry_type().is_file() {
            let mut output =
                File::create(&dest_path).map_err(|e| Error::Import(format!("Cannot create file: {}", e)))?;
            let size = std::io::copy(&mut entry, &mut output)
                .map_err(|e| Error::Import(format!("Extract failed: {}", e)))?;
            extracted_size += size;
            file_count += 1;

            if let Some(p) = progress {
                if file_count % 100 == 0 {
                    p.on_progress(extracted_size, total_size, "Extracting files");
                }
            }
        }
    }

    if let Some(p) = progress {
        p.on_complete(file_count);
    }

    Ok(SnapshotImportResult {
        collection: collection_name,
        files_extracted: file_count,
        bytes_extracted: extracted_size,
    })
}

/// Result of a snapshot import operation.
#[derive(Debug)]
pub struct SnapshotImportResult {
    pub collection: String,
    pub files_extracted: u64,
    pub bytes_extracted: u64,
}

/// Calculate total size of a directory recursively.
fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;

    if path.is_file() {
        return path
            .metadata()
            .map(|m| m.len())
            .map_err(|e| Error::Export(format!("Cannot read file size: {}", e)));
    }

    for entry in fs::read_dir(path).map_err(|e| Error::Export(format!("Cannot read directory: {}", e)))? {
        let entry = entry.map_err(|e| Error::Export(format!("Directory entry error: {}", e)))?;
        let path = entry.path();

        if path.is_file() {
            total += path.metadata().map(|m| m.len()).unwrap_or(0);
        } else if path.is_dir() {
            total += calculate_dir_size(&path)?;
        }
    }

    Ok(total)
}

/// Add bytes to a tar archive.
fn add_bytes_to_archive<W: Write>(
    archive: &mut tar::Builder<W>,
    name: &str,
    data: &[u8],
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_path(name).map_err(|e| Error::Export(format!("Path error: {}", e)))?;
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    header.set_cksum();

    archive
        .append(&header, data)
        .map_err(|e| Error::Export(format!("Archive append failed: {}", e)))?;

    Ok(())
}

/// Add a file to a tar archive.
fn add_file_to_archive<W: Write>(
    archive: &mut tar::Builder<W>,
    file_path: &Path,
    archive_path: &str,
    processed: &mut u64,
    total: u64,
    progress: Option<&dyn ExportProgress>,
) -> Result<()> {
    let mut file = File::open(file_path).map_err(|e| Error::Export(format!("Cannot open file: {}", e)))?;
    let size = file
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    let mut header = tar::Header::new_gnu();
    header
        .set_path(archive_path)
        .map_err(|e| Error::Export(format!("Path error: {}", e)))?;
    header.set_size(size);
    header.set_mode(0o644);
    header.set_mtime(
        file.metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    header.set_cksum();

    archive
        .append(&header, &mut file)
        .map_err(|e| Error::Export(format!("Archive append failed: {}", e)))?;

    *processed += size;
    if let Some(p) = progress {
        p.on_progress(*processed, total, "Archiving files");
    }

    Ok(())
}

/// Add a directory to a tar archive recursively.
fn add_directory_to_archive<W: Write>(
    archive: &mut tar::Builder<W>,
    dir_path: &Path,
    archive_prefix: &str,
    processed: &mut u64,
    total: u64,
    progress: Option<&dyn ExportProgress>,
) -> Result<()> {
    for entry in
        fs::read_dir(dir_path).map_err(|e| Error::Export(format!("Cannot read directory: {}", e)))?
    {
        let entry = entry.map_err(|e| Error::Export(format!("Directory entry error: {}", e)))?;
        let path = entry.path();
        let name = entry.file_name();
        let archive_path = format!("{}/{}", archive_prefix, name.to_string_lossy());

        if path.is_file() {
            add_file_to_archive(archive, &path, &archive_path, processed, total, progress)?;
        } else if path.is_dir() {
            add_directory_to_archive(archive, &path, &archive_path, processed, total, progress)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_calculate_dir_size() {
        let temp = TempDir::new().unwrap();
        let file1 = temp.path().join("file1.txt");
        let file2 = temp.path().join("file2.txt");

        fs::write(&file1, "hello").unwrap();
        fs::write(&file2, "world!").unwrap();

        let size = calculate_dir_size(temp.path()).unwrap();
        assert_eq!(size, 11); // "hello" (5) + "world!" (6)
    }
}
