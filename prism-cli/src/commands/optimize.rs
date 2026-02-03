use anyhow::{Context, Result};
use std::path::Path;
use std::time::Instant;
use tantivy::{Index, TantivyDocument};
use tantivy_common::ByteCount;

const HEAP_SIZE: usize = 300_000_000; // 300 MB

/// Format bytes as human-readable string
fn format_bytes(bytes: ByteCount) -> String {
    bytes.human_readable()
}

/// Format raw byte count
fn format_bytes_raw(bytes: u64) -> String {
    ByteCount::from(bytes).human_readable()
}

/// Run optimize command on a collection
pub fn run_optimize(data_dir: &Path, collection: &str, gc_only: bool) -> Result<()> {
    let index_path = data_dir.join("collections").join(collection).join("text");

    if !index_path.exists() {
        anyhow::bail!(
            "Collection '{}' not found at {:?}. Make sure the collection exists.",
            collection,
            index_path
        );
    }

    let index = Index::open_in_dir(&index_path)
        .with_context(|| format!("Failed to open index at {:?}", index_path))?;

    // Get initial stats
    let initial_segments = index.searchable_segment_ids()?;
    let initial_space = index.reader()?.searcher().space_usage()?.total();

    println!("Optimizing collection '{}'", collection);
    println!("  Initial segments: {}", initial_segments.len());
    println!("  Initial size:     {}", format_bytes(initial_space));
    println!();

    let start = Instant::now();

    if !gc_only && initial_segments.len() > 1 {
        println!("  Merging {} segments into 1...", initial_segments.len());

        let segment_meta = index
            .writer::<TantivyDocument>(HEAP_SIZE)?
            .merge(&initial_segments)
            .wait()
            .context("Merge failed")?;

        if let Some(meta) = segment_meta {
            println!("  Merge complete: new segment {}", meta.id().uuid_string());
        } else {
            println!("  Merge complete");
        }
    } else if !gc_only {
        println!("  Already optimized (single segment)");
    }

    // Always run garbage collection
    println!("  Running garbage collection...");

    Index::open_in_dir(&index_path)?
        .writer_with_num_threads::<TantivyDocument>(1, 40_000_000)?
        .garbage_collect_files()
        .wait()
        .context("Garbage collection failed")?;

    // Get final stats
    let final_segments = Index::open_in_dir(&index_path)?.searchable_segment_ids()?;
    let final_space = Index::open_in_dir(&index_path)?
        .reader()?
        .searcher()
        .space_usage()?
        .total();

    let elapsed = start.elapsed();
    let initial_bytes = initial_space.get_bytes();
    let final_bytes = final_space.get_bytes();
    let saved_bytes = if initial_bytes > final_bytes {
        initial_bytes - final_bytes
    } else {
        0
    };
    let pct_saved = if initial_bytes > 0 {
        (saved_bytes as f64 / initial_bytes as f64) * 100.0
    } else {
        0.0
    };

    println!();
    println!("Optimization complete:");
    println!("  Final segments: {}", final_segments.len());
    println!("  Final size:     {}", format_bytes(final_space));
    println!(
        "  Space saved:    {} ({:.1}%)",
        format_bytes_raw(saved_bytes),
        pct_saved
    );
    println!("  Time elapsed:   {:.2}s", elapsed.as_secs_f64());

    Ok(())
}
