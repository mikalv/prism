//! Restore (collection import) command implementation.

use anyhow::{Context, Result};
use prism::export::{
    portable::import_portable,
    snapshot::import_snapshot,
    types::{ConsoleProgress, ExportFormat},
};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Run the restore command (import from export).
pub async fn run_restore(
    data_dir: &Path,
    input: PathBuf,
    target_collection: Option<String>,
    format: Option<ExportFormat>,
    no_progress: bool,
) -> Result<()> {
    // Detect format from file extension if not specified
    let format = format.unwrap_or_else(|| {
        let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "zst" | "tar" => ExportFormat::Snapshot,
            _ => ExportFormat::Portable,
        }
    });

    println!("Restoring from '{}' in {} format", input.display(), format);

    let progress = if no_progress {
        None
    } else {
        Some(ConsoleProgress::new("Restore:"))
    };

    match format {
        ExportFormat::Portable => {
            restore_portable(
                data_dir,
                &input,
                target_collection.as_deref(),
                progress.as_ref(),
            )
            .await?;
        }
        ExportFormat::Snapshot => {
            restore_snapshot(
                data_dir,
                &input,
                target_collection.as_deref(),
                progress.as_ref(),
            )?;
        }
    }

    println!();
    println!("Restore complete!");

    Ok(())
}

async fn restore_portable(
    data_dir: &Path,
    input_path: &Path,
    target_collection: Option<&str>,
    progress: Option<&ConsoleProgress>,
) -> Result<()> {
    // Create backends
    let text_backend = Arc::new(
        prism::backends::TextBackend::new(data_dir).context("Failed to create text backend")?,
    );
    let vector_backend = Arc::new(
        prism::backends::VectorBackend::new(data_dir).context("Failed to create vector backend")?,
    );

    // Open input file
    let file = File::open(input_path).context("Cannot open input file")?;
    let reader = BufReader::new(file);

    // Import
    let progress_ref: Option<&dyn prism::export::ExportProgress> =
        progress.map(|p| p as &dyn prism::export::ExportProgress);

    let result = import_portable(
        text_backend,
        vector_backend,
        reader,
        target_collection,
        progress_ref,
    )
    .await?;

    println!();
    println!("Restored collection: {}", result.collection);
    println!("Documents imported: {}", result.documents_imported);
    println!(
        "Checksum verified: {}",
        if result.checksum_valid {
            "yes"
        } else {
            "FAILED"
        }
    );

    Ok(())
}

fn restore_snapshot(
    data_dir: &Path,
    input_path: &Path,
    target_collection: Option<&str>,
    progress: Option<&ConsoleProgress>,
) -> Result<()> {
    let progress_ref: Option<&dyn prism::export::ExportProgress> =
        progress.map(|p| p as &dyn prism::export::ExportProgress);

    let result = import_snapshot(data_dir, input_path, target_collection, progress_ref)?;

    println!();
    println!("Restored collection: {}", result.collection);
    println!("Files extracted: {}", result.files_extracted);
    println!("Bytes extracted: {}", result.bytes_extracted);

    Ok(())
}
