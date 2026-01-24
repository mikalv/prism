use clap::Parser;
use searchcore::api::ApiServer;
use searchcore::backends::text::TextBackend;
use searchcore::backends::vector::VectorBackend;
use searchcore::collection::CollectionManager;
use searchcore::config::Config;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "engraph-server")]
#[command(about = "engraph-core HTTP API server")]
struct Cli {
    /// Path to config file (default: ~/.engraph/config.toml)
    #[arg(long, short)]
    config: Option<PathBuf>,

    /// Data directory (overrides config)
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Server bind address (overrides config)
    #[arg(long)]
    bind_addr: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let mut config = if let Some(config_path) = &cli.config {
        Config::load_or_create(config_path)?
    } else if let Some(data_dir) = &cli.data_dir {
        Config::load_from(data_dir)?
    } else {
        Config::load()?
    };

    // Apply CLI overrides
    if let Some(data_dir) = cli.data_dir {
        config.storage.data_dir = data_dir;
    }
    if let Some(bind_addr) = cli.bind_addr {
        config.server.bind_addr = bind_addr;
    }

    // Initialize logging
    let log_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    if let Some(log_file) = &config.logging.file {
        // File logging
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)?;
        tracing_subscriber::fmt()
            .with_env_filter(log_filter)
            .with_writer(file)
            .init();
    } else {
        // Stderr logging
        tracing_subscriber::fmt()
            .with_env_filter(log_filter)
            .init();
    }

    // Ensure directories exist
    config.ensure_dirs()?;

    tracing::info!("Starting engraph-core server");
    tracing::info!("Data dir: {:?}", config.storage.data_dir);
    tracing::info!("Bind address: {}", config.server.bind_addr);

    // Create backends using config paths
    let text_backend = Arc::new(TextBackend::new(config.text_data_dir())?);
    let vector_backend = Arc::new(VectorBackend::new(config.vector_data_dir())?);

    // Create collection manager
    let manager = CollectionManager::new(config.schemas_dir(), text_backend, vector_backend)?;

    // Initialize all backends
    manager.initialize().await?;
    tracing::info!("Initialized {} collections", manager.list_collections().len());

    let manager = Arc::new(manager);

    // Create and start server
    let server = ApiServer::new(manager);
    server.serve(&config.server.bind_addr).await?;

    Ok(())
}
