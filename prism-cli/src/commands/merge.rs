//! Collection merge command — merge multiple collections into a new target collection.

use anyhow::{Context, Result};
use prism::backends::{GraphEdge, GraphNode};
use prism_storage::{LocalStorage, SegmentStorage};
use std::path::Path;
use std::sync::Arc;

/// Run the collection merge command: merge source collections into a target.
pub async fn run_merge(
    data_dir: &Path,
    schemas_dir: &Path,
    sources: &[String],
    target: &str,
) -> Result<()> {
    // Validate sources
    if sources.len() < 2 {
        anyhow::bail!("At least two source collections are required");
    }
    for src in sources {
        if src == target {
            anyhow::bail!("Source '{}' cannot be the same as target '{}'", src, target);
        }
    }

    // Init manager
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

    // Validate all source collections exist and have graph backends
    for src in sources {
        manager
            .graph_backend(src)
            .with_context(|| format!("Collection '{}' has no graph backend", src))?;
    }

    // Check target doesn't already exist
    if manager.graph_backend(target).is_some() {
        anyhow::bail!(
            "Target collection '{}' already exists. Choose a different name.",
            target
        );
    }

    // Collect all graph nodes + edges from sources
    let mut all_nodes: Vec<GraphNode> = Vec::new();
    let mut all_edges: Vec<GraphEdge> = Vec::new();

    for src in sources {
        let graph = manager.graph_backend(src).unwrap();
        let nodes = graph.list_nodes();
        let edges = graph.list_edges();
        println!(
            "  Source '{}': {} nodes, {} edges",
            src,
            nodes.len(),
            edges.len()
        );
        all_nodes.extend(nodes);
        all_edges.extend(edges);
    }

    // Create target schema from first source, adjusting name and setting num_shards=1
    let first_schema = manager
        .get_schema(&sources[0])
        .with_context(|| format!("Cannot read schema for '{}'", sources[0]))?;

    let mut target_schema = first_schema.clone();
    target_schema.collection = target.to_string();
    // Set graph to single shard since we're consolidating
    if let Some(ref mut graph_config) = target_schema.backends.graph {
        graph_config.num_shards = 1;
    }

    // Write target schema YAML to schemas_dir
    let schema_path = schemas_dir.join(format!("{}.yaml", target));
    if schema_path.exists() {
        anyhow::bail!("Schema file already exists: {}", schema_path.display());
    }
    let yaml = serde_yaml::to_string(&target_schema)
        .context("Failed to serialize target schema to YAML")?;
    std::fs::write(&schema_path, &yaml)
        .with_context(|| format!("Failed to write schema to {}", schema_path.display()))?;
    println!("  Created target schema: {}", schema_path.display());

    // Re-init manager to pick up the new target collection
    let text_backend2 = Arc::new(
        prism::backends::TextBackend::new(data_dir).context("Failed to create text backend")?,
    );
    let vector_backend2 = Arc::new(
        prism::backends::VectorBackend::new(data_dir).context("Failed to create vector backend")?,
    );
    let graph_storage2: Option<Arc<dyn SegmentStorage>> =
        Some(Arc::new(LocalStorage::new(data_dir)));

    let manager2 = prism::collection::CollectionManager::new(
        schemas_dir,
        text_backend2,
        vector_backend2,
        graph_storage2,
    )
    .context("Failed to re-initialize collection manager")?;
    manager2.initialize().await?;

    let target_graph = manager2
        .graph_backend(target)
        .context("Target collection graph backend not found after re-init")?;

    // Index graph nodes into target
    let start = std::time::Instant::now();
    let mut node_count = 0;
    for node in all_nodes {
        target_graph.add_node(node).await?;
        node_count += 1;
    }

    // Index edges into target
    let mut edge_count = 0;
    for edge in all_edges {
        target_graph.add_edge(edge).await?;
        edge_count += 1;
    }

    let stats = target_graph.stats();
    println!();
    println!("Merge complete into '{}':", target);
    println!("  Nodes: {} indexed", node_count);
    println!("  Edges: {} indexed", edge_count);
    println!(
        "  Final: {} nodes, {} edges",
        stats.node_count, stats.edge_count
    );
    println!("  Time:  {:.2}s", start.elapsed().as_secs_f64());

    Ok(())
}
