use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;

#[derive(Parser, Debug)]
#[command(name = "prism")]
#[command(about = "Prism CLI - hybrid search engine tools")]
#[command(version)]
struct Cli {
    /// Data directory (defaults to ./data)
    #[arg(long, short = 'd', global = true, default_value = "./data")]
    data_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Collection management commands
    #[command(subcommand)]
    Collection(CollectionCommands),

    /// Document operations
    #[command(subcommand)]
    Document(DocumentCommands),

    /// Index management commands
    #[command(subcommand)]
    Index(IndexCommands),

    /// Run performance benchmarks
    Benchmark {
        /// Collection name
        #[arg(short, long)]
        collection: String,

        /// File containing queries (one per line)
        #[arg(short, long)]
        queries: PathBuf,

        /// Number of times to repeat each query
        #[arg(short, long, default_value = "10")]
        repeat: usize,

        /// Number of warmup iterations
        #[arg(short, long, default_value = "3")]
        warmup: usize,

        /// Number of top results to fetch
        #[arg(short = 'k', long, default_value = "10")]
        top_k: usize,
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

#[derive(Subcommand, Debug)]
enum CollectionCommands {
    /// Inspect a collection's index structure and statistics
    Inspect {
        /// Collection name
        #[arg(short, long)]
        name: String,

        /// Show detailed per-segment breakdown
        #[arg(short, long)]
        verbose: bool,
    },

    /// List all collections
    List,

    /// Export a collection for backup or migration
    Export {
        /// Collection name
        #[arg(short, long)]
        name: String,

        /// Output file path (defaults to <collection>.<ext>)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Export format: portable (JSON, cross-version) or snapshot (binary, fast)
        #[arg(short, long, default_value = "portable")]
        format: String,

        /// Schemas directory path
        #[arg(long, default_value = "schemas")]
        schemas_dir: PathBuf,

        /// Disable progress output
        #[arg(long)]
        no_progress: bool,
    },

    /// Restore a collection from export
    Restore {
        /// Input file path
        #[arg(short, long)]
        input: PathBuf,

        /// Target collection name (overrides source name)
        #[arg(short, long)]
        target: Option<String>,

        /// Export format: portable or snapshot (auto-detected from extension if omitted)
        #[arg(short, long)]
        format: Option<String>,

        /// Disable progress output
        #[arg(long)]
        no_progress: bool,
    },
}

#[derive(Subcommand, Debug)]
enum DocumentCommands {
    /// Import documents from JSONL file or stdin
    Import {
        /// Collection name
        #[arg(short, long)]
        collection: String,

        /// Input JSONL file (omit for stdin)
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Prism API URL
        #[arg(long, default_value = "http://localhost:3080")]
        api_url: String,

        /// Batch size for imports
        #[arg(long, default_value = "100")]
        batch_size: usize,

        /// Disable progress output
        #[arg(long)]
        no_progress: bool,
    },

    /// Export documents to JSONL
    Export {
        /// Collection name
        #[arg(short, long)]
        collection: String,

        /// Output file (omit for stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum IndexCommands {
    /// Optimize index by merging segments and garbage collecting
    Optimize {
        /// Collection name
        #[arg(short, long)]
        collection: String,

        /// Only run garbage collection, skip segment merge
        #[arg(long)]
        gc_only: bool,
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
        Commands::Collection(cmd) => match cmd {
            CollectionCommands::Inspect { name, verbose } => {
                commands::run_inspect(&cli.data_dir, &name, verbose)?;
            }
            CollectionCommands::List => {
                list_collections(&cli.data_dir)?;
            }
            CollectionCommands::Export {
                name,
                output,
                format,
                schemas_dir,
                no_progress,
            } => {
                let export_format = format
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!(e))?;
                commands::run_export(
                    &cli.data_dir,
                    &schemas_dir,
                    &name,
                    output,
                    export_format,
                    no_progress,
                )
                .await?;
            }
            CollectionCommands::Restore {
                input,
                target,
                format,
                no_progress,
            } => {
                let export_format = format
                    .map(|f| f.parse())
                    .transpose()
                    .map_err(|e: String| anyhow::anyhow!(e))?;
                commands::run_restore(&cli.data_dir, input, target, export_format, no_progress)
                    .await?;
            }
        },

        Commands::Document(cmd) => match cmd {
            DocumentCommands::Import {
                collection,
                file,
                api_url,
                batch_size,
                no_progress,
            } => {
                let source = match file {
                    Some(path) => commands::import::DocumentSource::FromFile(path),
                    None => commands::import::DocumentSource::FromStdin,
                };
                commands::run_import(&api_url, &collection, source, batch_size, no_progress)
                    .await?;
            }
            DocumentCommands::Export { collection, output } => {
                tracing::info!("Exporting collection {} to {:?}", collection, output);
                tracing::warn!("Export implementation pending");
            }
        },

        Commands::Index(cmd) => match cmd {
            IndexCommands::Optimize {
                collection,
                gc_only,
            } => {
                commands::run_optimize(&cli.data_dir, &collection, gc_only)?;
            }
        },

        Commands::Benchmark {
            collection,
            queries,
            repeat,
            warmup,
            top_k,
        } => {
            commands::run_benchmark(&cli.data_dir, &collection, &queries, repeat, warmup, top_k)?;
        }

        Commands::CacheStats { path } => {
            tracing::info!("Cache stats for {}", path);
            tracing::warn!("Cache stats implementation pending");
        }

        Commands::CacheClear {
            path,
            older_than_days,
        } => {
            tracing::info!("Clearing cache at {}", path);
            if let Some(days) = older_than_days {
                tracing::info!("Only entries older than {} days", days);
            }
            tracing::warn!("Cache clear implementation pending");
        }
    }

    Ok(())
}

fn list_collections(data_dir: &std::path::Path) -> Result<()> {
    let collections_dir = data_dir.join("collections");

    if !collections_dir.exists() {
        println!(
            "No collections found (directory {:?} does not exist)",
            collections_dir
        );
        return Ok(());
    }

    let mut collections: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&collections_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                collections.push(name.to_string());
            }
        }
    }

    if collections.is_empty() {
        println!("No collections found");
    } else {
        collections.sort();
        println!("Collections:");
        for name in collections {
            println!("  - {}", name);
        }
    }

    Ok(())
}
