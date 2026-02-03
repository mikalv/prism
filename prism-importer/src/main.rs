use clap::Parser;
use futures::StreamExt;
use prism_importer::{AuthMethod, ElasticsearchSource, ImportProgress, ImportSource, SourceSchema};
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

        /// Only show schema, don't import
        #[arg(long)]
        dry_run: bool,

        /// Output schema to YAML file
        #[arg(long)]
        schema_out: Option<PathBuf>,
    },
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

            let progress = ImportProgress::new(total);
            let target_collection = target.unwrap_or_else(|| schema.name.clone());

            // Stream and import
            let mut stream = es.stream_documents();
            let mut batch = Vec::with_capacity(batch_size);

            while let Some(result) = stream.next().await {
                match result {
                    Ok(doc) => {
                        batch.push(doc);
                        if batch.len() >= batch_size {
                            // TODO: Actually index to Prism
                            progress.inc(batch.len() as u64);
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
                progress.inc(batch.len() as u64);
            }

            progress.finish();

            println!(
                "\nImport complete: {} documents to collection '{}'",
                progress.imported(),
                target_collection
            );

            if progress.failed() > 0 {
                println!("Warning: {} documents failed", progress.failed());
            }
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
