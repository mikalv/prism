use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "prism")]
#[command(about = "Prism CLI - hybrid search engine tools")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Discover schemas from existing Tantivy indexes
    Discover {
        /// Input directory with existing indexes
        #[arg(short, long)]
        input: String,

        /// Output directory for YAML schemas
        #[arg(short, long)]
        output: String,
    },

    /// Export documents from indexes to JSONL
    Export {
        /// Input directory with indexes
        #[arg(short, long)]
        input: String,

        /// Output directory for JSONL files
        #[arg(short, long)]
        output: String,

        /// Collections to export (comma-separated)
        #[arg(short, long)]
        collections: Option<String>,
    },

    /// Import JSONL files via HTTP API
    Import {
        /// Input directory with JSONL files
        #[arg(short, long)]
        input: String,

        /// Prism API URL
        #[arg(long, default_value = "http://localhost:3080")]
        api_url: String,

        /// Collections to import (comma-separated)
        #[arg(short, long)]
        collections: Option<String>,

        /// Batch size for imports
        #[arg(long, default_value = "100")]
        batch_size: usize,
    },

    /// Show cache statistics
    CacheStats {
        /// Path to cache database
        #[arg(short, long)]
        path: String,
    },

    /// Clear embedding cache
    CacheClear {
        /// Path to cache database
        #[arg(short, long)]
        path: String,

        /// Only clear entries older than N days
        #[arg(long)]
        older_than_days: Option<u32>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Discover { input, output } => {
            tracing::info!("Discovering schemas from {} -> {}", input, output);
            // TODO: prism::migration::discover(&input, &output)?;
            tracing::warn!("Discover implementation pending");
        }
        Commands::Export {
            input,
            output,
            collections,
        } => {
            tracing::info!("Exporting from {} -> {}", input, output);
            if let Some(c) = collections {
                tracing::info!("Collections: {}", c);
            }
            // TODO: prism::migration::export(&input, &output, collections)?;
            tracing::warn!("Export implementation pending");
        }
        Commands::Import {
            input,
            api_url,
            collections,
            batch_size,
        } => {
            tracing::info!("Importing from {} -> {} (batch={})", input, api_url, batch_size);
            if let Some(c) = collections {
                tracing::info!("Collections: {}", c);
            }
            // TODO: prism::migration::import(&input, &api_url, collections, batch_size).await?;
            tracing::warn!("Import implementation pending");
        }
        Commands::CacheStats { path } => {
            tracing::info!("Cache stats for {}", path);
            // TODO: prism::cache::stats(&path)?;
            tracing::warn!("Cache stats implementation pending");
        }
        Commands::CacheClear { path, older_than_days } => {
            tracing::info!("Clearing cache at {}", path);
            if let Some(days) = older_than_days {
                tracing::info!("Only entries older than {} days", days);
            }
            // TODO: prism::cache::clear(&path, older_than_days)?;
            tracing::warn!("Cache clear implementation pending");
        }
    }

    Ok(())
}
