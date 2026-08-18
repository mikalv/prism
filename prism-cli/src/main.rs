use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
mod client;
mod output;

#[derive(Parser, Debug)]
#[command(name = "prism")]
#[command(about = "Prism CLI - hybrid search engine tools")]
#[command(version)]
struct Cli {
    /// Data directory (defaults to ./data)
    #[arg(long, short = 'd', global = true, default_value = "./data")]
    data_dir: PathBuf,

    /// Prism server URL (env PRISM_URL)
    #[arg(long, global = true, env = "PRISM_URL")]
    url: Option<String>,

    /// API key for bearer auth (env PRISM_API_KEY)
    #[arg(long, global = true, env = "PRISM_API_KEY")]
    api_key: Option<String>,

    /// Output format: table or json (env PRISM_OUTPUT)
    #[arg(long, short = 'o', global = true, env = "PRISM_OUTPUT", default_value = "table")]
    output: String,

    /// Request timeout in seconds
    #[arg(long, global = true, default_value = "30")]
    timeout: u64,

    /// Skip TLS certificate verification (self-signed certs)
    #[arg(long, global = true)]
    insecure: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

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

    /// Cluster management commands (rolling upgrades, drain)
    #[command(subcommand)]
    Cluster(ClusterCommands),

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

    /// List collections on the server (API mode)
    Collections,

    /// Document operations (API mode)
    #[command(subcommand)]
    Doc(DocCommands),

    /// Search a collection (API mode)
    Search {
        /// Collection name
        collection: String,
        /// Query text
        query: String,
        /// Search mode: hybrid, vector, or text
        #[arg(long, default_value = "hybrid")]
        mode: String,
        /// Maximum results
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Text-engine weight (requires --vector-weight too)
        #[arg(long)]
        text_weight: Option<f32>,
        /// Vector-engine weight (requires --text-weight too)
        #[arg(long)]
        vector_weight: Option<f32>,
    },

    /// Re-embed one or more collections (patterns allowed) (API mode)
    Reindex {
        /// Collection names or glob patterns (e.g. 'idx_*')
        #[arg(required = true)]
        collections: Vec<String>,
        /// Embedding batch size (1-1000)
        #[arg(long, default_value = "100")]
        batch_size: usize,
    },
    /// Schema operations (API mode)
    Schema {
        #[command(subcommand)]
        cmd: SchemaCommands,
    },
    /// Encrypted backup of a collection — writes to a SERVER-side path (API mode)
    Backup {
        collection: String,
        /// Output file path ON THE SERVER
        output_path: String,
        /// Hex encryption key (64 chars). If omitted, one is generated and printed to stderr.
        #[arg(long)]
        key: Option<String>,
    },
    /// Restore a collection from an encrypted backup (API mode)
    Restore {
        /// Input file path ON THE SERVER
        input_path: String,
        #[arg(long)]
        key: String,
        /// Rename the restored collection
        #[arg(long)]
        target_collection: Option<String>,
    },
    /// Generate an encryption key for backup/restore (API mode)
    BackupKey,
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

    /// Detach a collection from a running server (snapshot + unload)
    Detach {
        /// Collection name
        #[arg(short, long)]
        name: String,

        /// Output snapshot file path
        #[arg(short, long)]
        output: PathBuf,

        /// Prism API URL
        #[arg(long, default_value = "http://localhost:3080")]
        api_url: String,

        /// Delete on-disk data after detaching
        #[arg(long)]
        delete_data: bool,
    },

    /// Attach a collection from a snapshot into a running server
    Attach {
        /// Input snapshot file path
        #[arg(short, long)]
        input: PathBuf,

        /// Target collection name (overrides name in snapshot)
        #[arg(short, long)]
        target: Option<String>,

        /// Prism API URL
        #[arg(long, default_value = "http://localhost:3080")]
        api_url: String,
    },

    /// Merge all graph shards into one (consolidation)
    GraphMerge {
        /// Collection name
        #[arg(short, long)]
        name: String,

        /// Schemas directory path
        #[arg(long, default_value = "schemas")]
        schemas_dir: PathBuf,
    },

    /// Migrate a collection between Prism instances via HTTP API
    Migrate {
        /// Source Prism API URL
        #[arg(long)]
        source_url: String,

        /// Target Prism API URL
        #[arg(long)]
        target_url: String,

        /// Collection name to migrate
        #[arg(short, long)]
        name: String,

        /// Target collection name (defaults to same as source)
        #[arg(short, long)]
        target: Option<String>,

        /// Batch size for scroll/index operations
        #[arg(long, default_value = "100")]
        batch_size: usize,
    },

    /// Merge multiple collections into a new target collection
    Merge {
        /// Source collection names (at least 2)
        #[arg(short, long, num_args = 2..)]
        source: Vec<String>,

        /// Target collection name
        #[arg(short, long)]
        target: String,

        /// Schemas directory path
        #[arg(long, default_value = "schemas")]
        schemas_dir: PathBuf,
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

#[derive(Subcommand, Debug)]
enum DocCommands {
    /// Fetch a document by id
    Get {
        /// Collection name
        collection: String,
        /// Document id
        id: String,
    },
    /// Index one JSON document from a file, stdin ('-'), or an inline JSON object
    Index {
        /// Collection name
        collection: String,
        /// Input file, '-' for stdin, or an inline JSON object
        file: String,
    },
    /// Delete a document by id
    Delete {
        /// Collection name
        collection: String,
        /// Document id
        id: String,
    },
    /// Bulk-import JSONL documents from a file or stdin ('-')
    Bulk {
        /// Collection name
        collection: String,
        /// Input JSONL file or '-' for stdin
        file: String,
        /// Documents per POST batch
        #[arg(long, default_value = "100")]
        batch_size: usize,
    },
}

#[derive(Subcommand, Debug)]
enum SchemaCommands {
    /// Print a collection's schema
    Get {
        /// Collection name
        collection: String,
    },
    /// Report schema issues across all collections
    Lint,
}

#[derive(Subcommand, Debug)]
enum ClusterCommands {
    /// Show upgrade status for all cluster nodes
    UpgradeStatus {
        /// Prism API URL
        #[arg(long, default_value = "http://localhost:3080")]
        api_url: String,
    },

    /// Drain a node (stop routing new queries to it)
    Drain {
        /// Node ID to drain
        #[arg(long)]
        node: String,

        /// Prism API URL
        #[arg(long, default_value = "http://localhost:3080")]
        api_url: String,
    },

    /// Undrain a node (resume routing queries to it)
    Undrain {
        /// Node ID to undrain
        #[arg(long)]
        node: String,

        /// Prism API URL
        #[arg(long, default_value = "http://localhost:3080")]
        api_url: String,
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

    let opts = commands::api::ApiOpts {
        url: cli.url.clone(),
        api_key: cli.api_key.clone(),
        timeout: cli.timeout,
        insecure: cli.insecure,
        json: cli.output == "json",
    };

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
                let export_format = format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
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
            CollectionCommands::Detach {
                name,
                output,
                api_url,
                delete_data,
            } => {
                commands::run_detach(&api_url, &name, output, delete_data).await?;
            }
            CollectionCommands::Attach {
                input,
                target,
                api_url,
            } => {
                commands::run_attach(&api_url, input, target).await?;
            }
            CollectionCommands::Migrate {
                source_url,
                target_url,
                name,
                target,
                batch_size,
            } => {
                commands::run_migrate(
                    &source_url,
                    &target_url,
                    &name,
                    target.as_deref(),
                    batch_size,
                )
                .await?;
            }
            CollectionCommands::GraphMerge { name, schemas_dir } => {
                commands::run_graph_merge(&cli.data_dir, &schemas_dir, &name).await?;
            }
            CollectionCommands::Merge {
                source,
                target,
                schemas_dir,
            } => {
                commands::run_merge(&cli.data_dir, &schemas_dir, &source, &target).await?;
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

        Commands::Cluster(cmd) => match cmd {
            ClusterCommands::UpgradeStatus { api_url } => {
                commands::run_upgrade_status(&api_url).await?;
            }
            ClusterCommands::Drain { node, api_url } => {
                commands::run_drain(&api_url, &node).await?;
            }
            ClusterCommands::Undrain { node, api_url } => {
                commands::run_undrain(&api_url, &node).await?;
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

        Commands::Collections => {
            let code = commands::api::run_collections(&opts).await?;
            std::process::exit(code);
        }

        Commands::Doc(cmd) => match cmd {
            DocCommands::Get { collection, id } => {
                let code = commands::api::run_doc_get(&opts, &collection, &id).await?;
                std::process::exit(code);
            }
            DocCommands::Index { collection, file } => {
                let code = commands::api::run_doc_index(&opts, &collection, &file).await?;
                std::process::exit(code);
            }
            DocCommands::Delete { collection, id } => {
                let code = commands::api::run_doc_delete(&opts, &collection, &id).await?;
                std::process::exit(code);
            }
            DocCommands::Bulk { collection, file, batch_size } => {
                let code = commands::api::run_doc_bulk(&opts, &collection, &file, batch_size).await?;
                std::process::exit(code);
            }
        },

        Commands::Search { collection, query, mode, limit, text_weight, vector_weight } => {
            let weights = match (text_weight, vector_weight) {
                (Some(t), Some(v)) => Some((t, v)),
                (None, None) => None,
                _ => anyhow::bail!("--text-weight and --vector-weight must be set together"),
            };
            let code = commands::api::run_search(&opts, &collection, &query, &mode, limit, weights).await?;
            std::process::exit(code);
        }

        Commands::Reindex { collections, batch_size } => {
            let code = commands::api::run_reindex(&opts, collections, batch_size).await?;
            std::process::exit(code);
        }

        Commands::Schema { cmd } => match cmd {
            SchemaCommands::Get { collection } => {
                let code = commands::api::run_schema_get(&opts, &collection).await?;
                std::process::exit(code);
            }
            SchemaCommands::Lint => {
                let code = commands::api::run_schema_lint(&opts).await?;
                std::process::exit(code);
            }
        },

        Commands::Backup { collection, output_path, key } => {
            let code = commands::api::run_backup(&opts, &collection, &output_path, key.as_deref()).await?;
            std::process::exit(code);
        }

        Commands::Restore { input_path, key, target_collection } => {
            let code = commands::api::run_restore(&opts, &input_path, &key, target_collection).await?;
            std::process::exit(code);
        }

        Commands::BackupKey => {
            let code = commands::api::run_backup_keygen(&opts).await?;
            std::process::exit(code);
        }
    }

    Ok(())
}

fn list_collections(data_dir: &std::path::Path) -> Result<()> {
    // Try direct path first, then legacy collections/ subdir
    let collections_dir = if data_dir.exists() {
        data_dir.to_path_buf()
    } else {
        data_dir.join("collections")
    };

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
                // Skip internal directories
                if name == "cache" || name == "data" {
                    continue;
                }
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
