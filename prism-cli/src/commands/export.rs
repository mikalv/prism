//! Export command implementation.

use anyhow::{Context, Result};
use prism::export::{
    portable::export_portable,
    snapshot::export_snapshot,
    types::{ConsoleProgress, ExportFormat},
};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Run the export command.
pub async fn run_export(
    data_dir: &Path,
    schemas_dir: &Path,
    collection: &str,
    output: Option<PathBuf>,
    format: ExportFormat,
    no_progress: bool,
) -> Result<()> {
    println!("Exporting collection '{}' in {} format", collection, format);

    let output_path = output.unwrap_or_else(|| {
        let ext = match format {
            ExportFormat::Portable => "jsonl",
            ExportFormat::Snapshot => "tar.zst",
        };
        PathBuf::from(format!("{}.{}", collection, ext))
    });

    let progress = if no_progress {
        None
    } else {
        Some(ConsoleProgress::new("Export:"))
    };

    match format {
        ExportFormat::Portable => {
            export_portable_collection(
                data_dir,
                schemas_dir,
                collection,
                &output_path,
                progress.as_ref(),
            )
            .await?;
        }
        ExportFormat::Snapshot => {
            export_snapshot_collection(data_dir, collection, &output_path, progress.as_ref())?;
        }
    }

    println!();
    println!("Export complete: {}", output_path.display());

    Ok(())
}

async fn export_portable_collection(
    data_dir: &Path,
    schemas_dir: &Path,
    collection: &str,
    output_path: &Path,
    progress: Option<&ConsoleProgress>,
) -> Result<()> {
    // Create backends
    let text_backend = Arc::new(
        prism::backends::TextBackend::new(data_dir).context("Failed to create text backend")?,
    );
    let vector_backend = Arc::new(
        prism::backends::VectorBackend::new(data_dir).context("Failed to create vector backend")?,
    );

    // Create collection manager
    let manager =
        prism::collection::CollectionManager::new(schemas_dir, text_backend, vector_backend, None)
            .context("Failed to create collection manager")?;
    manager.initialize().await?;

    // Create output file
    let file = File::create(output_path).context("Cannot create output file")?;
    let writer = BufWriter::new(file);

    // Export
    let progress_ref: Option<&dyn prism::export::ExportProgress> =
        progress.map(|p| p as &dyn prism::export::ExportProgress);

    let metadata = export_portable(&manager, collection, writer, progress_ref).await?;

    println!();
    println!("Exported {} documents", metadata.document_count);
    if let Some(checksum) = metadata.checksum {
        println!("Checksum: {}", checksum);
    }

    Ok(())
}

fn export_snapshot_collection(
    data_dir: &Path,
    collection: &str,
    output_path: &Path,
    progress: Option<&ConsoleProgress>,
) -> Result<()> {
    let progress_ref: Option<&dyn prism::export::ExportProgress> =
        progress.map(|p| p as &dyn prism::export::ExportProgress);

    let metadata = export_snapshot(data_dir, collection, output_path, progress_ref)?;

    println!();
    println!("Exported {} bytes", metadata.size_bytes);

    Ok(())
}
