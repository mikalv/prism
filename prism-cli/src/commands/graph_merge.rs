//! Graph shard merge command — consolidate all graph shards into shard 0.

use anyhow::{Context, Result};
use prism_storage::{LocalStorage, SegmentStorage};
use std::path::Path;
use std::sync::Arc;

/// Run the graph-merge command: merge all graph shards in a collection into one.
pub async fn run_graph_merge(data_dir: &Path, schemas_dir: &Path, collection: &str) -> Result<()> {
    let text_backend = Arc::new(
        prism::backends::TextBackend::new(data_dir).context("Failed to create text backend")?,
    );
    let vector_backend = Arc::new(
        prism::backends::VectorBackend::new(data_dir).context("Failed to create vector backend")?,
    );
    let graph_storage: Option<Arc<dyn SegmentStorage>> =
        Some(Arc::new(LocalStorage::new(data_dir)));

    let manager = prism::collection::CollectionManager::new(
        schemas_dir,
        text_backend,
        vector_backend,
        graph_storage,
    )
    .context("Failed to create collection manager")?;
    manager.initialize().await?;

    let graph = manager
        .graph_backend(collection)
        .context("Collection has no graph backend")?;

    let before = graph.stats();
    println!("Merging graph shards for '{}'", collection);
    println!(
        "  Before: {} nodes, {} edges across {} shards",
        before.node_count,
        before.edge_count,
        graph.num_shards()
    );

    let start = std::time::Instant::now();
    let (nodes, edges) = graph.merge_all_shards().await?;

    println!("  After:  {} nodes, {} edges in shard 0", nodes, edges);
    println!("  Time:   {:.2}s", start.elapsed().as_secs_f64());

    Ok(())
}
