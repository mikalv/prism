use clap::{Parser, Subcommand};
use searchcore::migration::{DataExporter, DataImporter, SchemaDiscoverer};
use searchcore::Result;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "engraph-migrate")]
#[command(about = "Migration tool for engraph-core")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Graph operations (add nodes/edges, bfs queries)
    Graph {
        #[command(subcommand)]
        action: GraphAction,
    },
    /// Discover schemas from old engraph indexes
    Discover {
        /// Path to old engraph data directory
        #[arg(long)]
        input: PathBuf,

        /// Output directory for YAML schemas
        #[arg(long)]
        output: PathBuf,
    },

    /// Export data from old engraph indexes to JSONL
    Export {
        /// Path to old engraph data directory
        #[arg(long)]
        input: PathBuf,

        /// Output directory for JSONL files
        #[arg(long)]
        output: PathBuf,

        /// Comma-separated list of collections to export
        #[arg(long)]
        collections: String,
    },

    /// Import data from JSONL files to new engraph-core
    Import {
        /// Directory containing JSONL files
        #[arg(long)]
        input: PathBuf,

        /// Base URL of engraph-core HTTP API
        #[arg(long, default_value = "http://localhost:8080")]
        api_url: String,

        /// Comma-separated list of collections to import
        #[arg(long)]
        collections: String,

        /// Batch size for import (number of documents per request)
        #[arg(long, default_value = "100")]
        batch_size: usize,
    },
    /// Lint schemas using server HTTP API or locally
    LintSchemas {
        /// Base URL of engraph-core HTTP API
        #[arg(long, default_value = "http://localhost:8080")]
        api_url: String,

        /// Run linting locally against a schemas directory instead of calling the API
        #[arg(long)]
        local: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum GraphAction {
    /// Add a node to a collection graph
    AddNode {
        /// Collection name
        #[arg(long)]
        collection: String,
        /// Node ID
        #[arg(long)]
        node_id: String,
        /// Node type
        #[arg(long)]
        node_type: String,
        /// Title
        #[arg(long)]
        title: String,
        /// JSON payload string
        #[arg(long, default_value = "{}")]
        payload: String,
        /// API base URL
        #[arg(long, default_value = "http://localhost:8080")]
        api_url: String,
    },

    /// Add an edge to a collection graph
    AddEdge {
        /// Collection name
        #[arg(long)]
        collection: String,
        /// From node ID
        #[arg(long)]
        from: String,
        /// To node ID
        #[arg(long)]
        to: String,
        /// Edge type
        #[arg(long)]
        edge_type: String,
        /// Weight (optional)
        #[arg(long)]
        weight: Option<f32>,
        /// API base URL
        #[arg(long, default_value = "http://localhost:8080")]
        api_url: String,
    },

    /// Run a bfs query against a collection graph
    Bfs {
        /// Collection name
        #[arg(long)]
        collection: String,
        /// Start node id
        #[arg(long)]
        start: String,
        /// Edge type
        #[arg(long)]
        edge_type: String,
        /// Max depth
        #[arg(long, default_value = "3")]
        max_depth: u32,
        /// API base URL
        #[arg(long, default_value = "http://localhost:8080")]
        api_url: String,
    },

    /// Compute weighted shortest path
    ShortestPath {
        /// Collection name
        #[arg(long)]
        collection: String,
        /// Start node id
        #[arg(long)]
        start: String,
        /// Target node id
        #[arg(long)]
        target: String,
        /// Optional comma-separated allowed edge types
        #[arg(long)]
        edge_types: Option<String>,
        /// API base URL
        #[arg(long, default_value = "http://localhost:8080")]
        api_url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Discover { input, output } => {
            println!("Discovering schemas from: {}", input.display());

            let discoverer = SchemaDiscoverer::new(&input);
            let schemas = discoverer.discover_all()?;

            println!("Found {} collections", schemas.len());

            discoverer.write_schemas(&output, &schemas)?;

            println!("Wrote schemas to: {}", output.display());

            Ok(())
        }

        Commands::Export {
            input,
            output,
            collections,
        } => {
            let collection_list: Vec<String> = collections
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            println!(
                "Exporting {} collections from: {}",
                collection_list.len(),
                input.display()
            );

            let exporter = DataExporter::new(&input);
            exporter.export_all(&collection_list, &output)?;

            println!("Export complete!");

            Ok(())
        }

        Commands::Import {
            input,
            api_url,
            collections,
            batch_size,
        } => {
            let collection_list: Vec<String> = collections
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            println!(
                "Importing {} collections from: {}",
                collection_list.len(),
                input.display()
            );
            println!("Target API: {}", api_url);
            println!("Batch size: {}", batch_size);

            let importer = DataImporter::new(&input, api_url).with_batch_size(batch_size);
            let results = importer.import_all(&collection_list).await?;

            println!("\nImport complete!");
            println!("Summary:");
            for (collection, count) in results {
                println!("  {} → {} documents", collection, count);
            }

            Ok(())
        }

        Commands::LintSchemas { api_url, local } => {
            if let Some(path) = local {
                println!("Linting schemas locally in: {}", path.display());
                let loader = searchcore::schema::loader::SchemaLoader::new(path);
                let schemas = loader.load_all()?;
                let issues = searchcore::schema::loader::SchemaLoader::lint_all(&schemas);
                println!("Found {} collections", schemas.len());
                println!("Lint results:\n{}", serde_json::to_string_pretty(&issues)?);
                return Ok(());
            }

            println!("Linting schemas via API: {}", api_url);
            let client = reqwest::Client::new();
            let resp = client
                .get(format!("{}/admin/lint-schemas", api_url.trim_end_matches('/')))
                .send()
                .await?;
            if !resp.status().is_success() {
                println!("Server returned error: {}", resp.status());
                return Ok(());
            }
            let json: serde_json::Value = resp.json().await?;
            println!("Lint results:\n{}", serde_json::to_string_pretty(&json)?);
            Ok(())
        }

        Commands::Graph { action } => {
            match action {
                GraphAction::AddNode { collection, node_id, node_type, title, payload, api_url } => {
                    let client = reqwest::Client::new();
                    let url = format!("{}/collections/{}/graph/add_node", api_url.trim_end_matches('/'), collection);
                    let parsed_payload = serde_json::from_str::<serde_json::Value>(&payload).unwrap_or(serde_json::json!({}));
                    let body = serde_json::json!({"node_id": node_id, "node_type": node_type, "title": title, "payload": parsed_payload});
                    let resp = client.post(&url).json(&body).send().await?;
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    println!("HTTP {}", status);
                    if status.is_success() {
                        if !text.is_empty() {
                            let j: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.clone()));
                            println!("Response:\n{}", serde_json::to_string_pretty(&j)?);
                        } else {
                            println!("Created node {}", node_id);
                        }
                    } else {
                        println!("Error: {}", text);
                    }
                    Ok(())
                }
                GraphAction::AddEdge { collection, from, to, edge_type, weight, api_url } => {
                    let client = reqwest::Client::new();
                    let url = format!("{}/collections/{}/graph/add_edge", api_url.trim_end_matches('/'), collection);
                    let body = serde_json::json!({"from": from, "to": to, "edge_type": edge_type, "weight": weight});
                    let resp = client.post(&url).json(&body).send().await?;
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    println!("HTTP {}", status);
                    if status.is_success() {
                        println!("Edge created: {} -> {} ({})", from, to, edge_type);
                        if !text.is_empty() {
                            let j: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.clone()));
                            println!("Response:\n{}", serde_json::to_string_pretty(&j)?);
                        }
                    } else {
                        println!("Error: {}", text);
                    }
                    Ok(())
                }
                GraphAction::Bfs { collection, start, edge_type, max_depth, api_url } => {
                    let client = reqwest::Client::new();
                    let url = format!("{}/collections/{}/graph/bfs", api_url.trim_end_matches('/'), collection);
                    let body = serde_json::json!({"start": start, "edge_type": edge_type, "max_depth": max_depth});
                    let resp = client.post(&url).json(&body).send().await?;
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    println!("HTTP {}", status);
                    if status.is_success() {
                        let j: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.clone()));
                        println!("Paths:\n{}", serde_json::to_string_pretty(&j)?);
                    } else {
                        println!("Error: {}", text);
                    }
                    Ok(())
                }
                GraphAction::ShortestPath { collection, start, target, edge_types, api_url } => {
                    let client = reqwest::Client::new();
                    let url = format!("{}/collections/{}/graph/shortest_path", api_url.trim_end_matches('/'), collection);
                    let body = serde_json::json!({"start": start, "target": target, "edge_types": edge_types});
                    let resp = client.post(&url).json(&body).send().await?;
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    println!("HTTP {}", status);
                    if status.is_success() {
                        let j: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.clone()));
                        println!("Path:\n{}", serde_json::to_string_pretty(&j)?);
                    } else {
                        println!("Error: {}", text);
                    }
                    Ok(())
                }
            }
        }
    }
}
