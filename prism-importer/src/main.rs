use clap::Parser;
use futures::StreamExt;
use prism_importer::{
    AuthMethod, ElasticsearchSource, ImportProgress, ImportSource, SourceSchema, WikipediaSource,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use url::Url;

#[derive(Parser)]
#[command(name = "prism-import")]
#[command(about = "Import data from external search engines into Prism")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Import from Elasticsearch
    Es {
        /// Elasticsearch URL (e.g., http://localhost:9200)
        #[arg(long)]
        source: Url,

        /// Index name or pattern
        #[arg(long)]
        index: String,

        /// Target Prism collection name (defaults to index name)
        #[arg(long)]
        target: Option<String>,

        /// Username for basic auth
        #[arg(long)]
        user: Option<String>,

        /// Password for basic auth
        #[arg(long)]
        password: Option<String>,

        /// API key for authentication
        #[arg(long)]
        api_key: Option<String>,

        /// Batch size for scroll API
        #[arg(long, default_value = "1000")]
        batch_size: usize,

        /// Prism server URL
        #[arg(long, default_value = "http://localhost:3080")]
        prism_url: String,

        /// Only show schema, don't import
        #[arg(long)]
        dry_run: bool,

        /// Output schema to YAML file
        #[arg(long)]
        schema_out: Option<PathBuf>,
    },

    /// Import from Wikipedia XML dump
    Wiki {
        /// Path to Wikipedia XML dump (.xml.bz2 or .xml)
        #[arg(long)]
        dump: PathBuf,

        /// Prism server URL
        #[arg(long, default_value = "http://localhost:3080")]
        prism_url: String,

        /// Target collection name
        #[arg(long, default_value = "wikipedia")]
        collection: String,

        /// Batch size for indexing
        #[arg(long, default_value = "500")]
        batch_size: usize,

        /// Max articles to import (0 = all)
        #[arg(long, default_value = "0")]
        max_docs: usize,

        /// Only show schema, don't import
        #[arg(long)]
        dry_run: bool,
    },
}

/// POST a batch of documents to Prism's index API
async fn index_batch(
    client: &reqwest::Client,
    prism_url: &str,
    collection: &str,
    batch: &[prism_importer::sources::traits::SourceDocument],
) -> anyhow::Result<(usize, usize)> {
    // Convert SourceDocuments to Prism's expected format
    let documents: Vec<serde_json::Value> = batch
        .iter()
        .map(|doc| {
            let fields: HashMap<String, serde_json::Value> =
                if let serde_json::Value::Object(map) = &doc.fields {
                    map.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                } else {
                    HashMap::new()
                };
            // Ensure the id is part of the document
            serde_json::json!({
                "id": doc.id,
                "fields": fields,
            })
        })
        .collect();

    let url = format!(
        "{}/collections/{}/documents",
        prism_url.trim_end_matches('/'),
        collection
    );

    let response = client
        .post(&url)
        .json(&serde_json::json!({ "documents": documents }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Index request failed ({}): {}", status, body);
    }

    #[derive(Deserialize)]
    struct IndexResponse {
        indexed: usize,
        #[serde(default)]
        failed: usize,
    }

    let resp: IndexResponse = response.json().await?;
    Ok((resp.indexed, resp.failed))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Es {
            source,
            index,
            target,
            user,
            password,
            api_key,
            batch_size,
            prism_url,
            dry_run,
            schema_out,
        } => {
            let auth = match (user, password, api_key) {
                (Some(u), Some(p), _) => AuthMethod::Basic {
                    user: u,
                    password: p,
                },
                (_, _, Some(key)) => AuthMethod::ApiKey(key),
                _ => AuthMethod::None,
            };

            let es = ElasticsearchSource::new(source.clone(), index.clone(), auth)?
                .with_batch_size(batch_size);

            println!("Connecting to {}...", source);

            // Fetch schema
            let schema = es.fetch_schema().await?;
            println!("\nSchema for '{}':", schema.name);
            print_schema(&schema);

            // Write schema if requested
            if let Some(path) = schema_out {
                let yaml = serde_yaml::to_string(&schema)?;
                std::fs::write(&path, yaml)?;
                println!("\nSchema written to {}", path.display());
            }

            if dry_run {
                println!("\n--dry-run specified, skipping import.");
                return Ok(());
            }

            // Count documents
            let total = es.count_documents().await?;
            println!("\nImporting {} documents...\n", total);

            let client = reqwest::Client::new();
            let progress = ImportProgress::new(total);
            let target_collection = target.unwrap_or_else(|| schema.name.clone());

            // Stream and import
            let mut stream = es.stream_documents();
            let mut batch = Vec::with_capacity(batch_size);
            let mut total_failed = 0u64;

            while let Some(result) = stream.next().await {
                match result {
                    Ok(doc) => {
                        batch.push(doc);
                        if batch.len() >= batch_size {
                            match index_batch(&client, &prism_url, &target_collection, &batch).await
                            {
                                Ok((_, failed)) => {
                                    total_failed += failed as u64;
                                    progress.inc(batch.len() as u64);
                                }
                                Err(e) => {
                                    tracing::error!("Batch index error: {}", e);
                                    progress.inc_failed(batch.len() as u64);
                                }
                            }
                            batch.clear();
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Document error: {}", e);
                        progress.inc_failed(1);
                    }
                }
            }

            // Final batch
            if !batch.is_empty() {
                match index_batch(&client, &prism_url, &target_collection, &batch).await {
                    Ok((_, failed)) => {
                        total_failed += failed as u64;
                        progress.inc(batch.len() as u64);
                    }
                    Err(e) => {
                        tracing::error!("Final batch index error: {}", e);
                        progress.inc_failed(batch.len() as u64);
                    }
                }
            }

            progress.finish();

            println!(
                "\nImport complete: {} documents to collection '{}'",
                progress.imported(),
                target_collection
            );

            if progress.failed() > 0 || total_failed > 0 {
                println!(
                    "Warning: {} stream failures, {} index failures",
                    progress.failed(),
                    total_failed
                );
            }
        }

        Commands::Wiki {
            dump,
            prism_url,
            collection,
            batch_size,
            max_docs,
            dry_run,
        } => {
            let wiki = WikipediaSource::new(dump.clone())?;

            println!("Wikipedia dump: {}", dump.display());

            // Fetch schema
            let schema = wiki.fetch_schema().await?;
            println!("\nSchema:");
            print_schema(&schema);

            // Estimate count
            let estimated = wiki.count_documents().await?;
            let total = if max_docs > 0 {
                std::cmp::min(estimated, max_docs as u64)
            } else {
                estimated
            };
            println!("\nEstimated articles: ~{}", estimated);
            if max_docs > 0 {
                println!("Max articles to import: {}", max_docs);
            }

            if dry_run {
                println!("\n--dry-run specified, skipping import.");
                return Ok(());
            }

            println!("\nImporting to '{}' at {}...\n", collection, prism_url);

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?;
            let progress = ImportProgress::new(total);

            let mut stream = wiki.stream_documents();
            let mut batch = Vec::with_capacity(batch_size);
            let mut doc_count = 0u64;
            let mut total_indexed = 0u64;
            let mut total_failed = 0u64;

            while let Some(result) = stream.next().await {
                if max_docs > 0 && doc_count >= max_docs as u64 {
                    break;
                }

                match result {
                    Ok(doc) => {
                        batch.push(doc);
                        doc_count += 1;

                        if batch.len() >= batch_size {
                            match index_batch(&client, &prism_url, &collection, &batch).await {
                                Ok((indexed, failed)) => {
                                    total_indexed += indexed as u64;
                                    total_failed += failed as u64;
                                    progress.inc(batch.len() as u64);
                                }
                                Err(e) => {
                                    tracing::error!("Batch index error: {}", e);
                                    progress.inc_failed(batch.len() as u64);
                                }
                            }
                            batch.clear();
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Document error: {}", e);
                        progress.inc_failed(1);
                        doc_count += 1;
                    }
                }
            }

            // Final batch
            if !batch.is_empty() {
                match index_batch(&client, &prism_url, &collection, &batch).await {
                    Ok((indexed, failed)) => {
                        total_indexed += indexed as u64;
                        total_failed += failed as u64;
                        progress.inc(batch.len() as u64);
                    }
                    Err(e) => {
                        tracing::error!("Final batch index error: {}", e);
                        progress.inc_failed(batch.len() as u64);
                    }
                }
            }

            progress.finish();

            println!("\nImport complete:");
            println!("  Articles processed: {}", doc_count);
            println!("  Documents indexed:  {}", total_indexed);
            println!("  Index failures:     {}", total_failed);
            println!("  Stream failures:    {}", progress.failed());
            println!("  Collection:         '{}'", collection);
        }
    }

    Ok(())
}

fn print_schema(schema: &SourceSchema) {
    println!("  Fields:");
    for field in &schema.fields {
        let dims = field
            .vector_dims
            .map(|d| format!(" (dims={})", d))
            .unwrap_or_default();
        println!("    - {}: {}{}", field.name, field.field_type, dims);
    }
}
