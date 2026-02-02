use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "prism-server")]
#[command(about = "Prism hybrid search server")]
#[command(version)]
struct Args {
    /// Configuration file path
    #[arg(short, long, default_value = "prism.toml")]
    config: String,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on
    #[arg(short, long, default_value = "3080")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,prism=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    tracing::info!("Starting Prism server on {}:{}", args.host, args.port);
    tracing::info!("Config file: {}", args.config);

    // Load config
    let config = prism::config::Config::load_or_create(std::path::Path::new(&args.config))?;
    config.ensure_dirs()?;

    let addr = format!("{}:{}", args.host, args.port);

    // Create backends
    let text_backend = std::sync::Arc::new(prism::backends::text::TextBackend::new(
        &config.storage.data_dir,
    )?);
    let vector_backend = std::sync::Arc::new(prism::backends::VectorBackend::new(
        &config.storage.data_dir,
    )?);

    // Create collection manager
    let manager = std::sync::Arc::new(prism::collection::CollectionManager::new(
        config.schemas_dir(),
        text_backend,
        vector_backend,
    )?);
    manager.initialize().await?;

    // Load ingest pipelines
    let config_dir = std::path::Path::new(&args.config)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let pipelines_dir = config_dir.join("conf/pipelines");
    let pipeline_registry = prism::pipeline::registry::PipelineRegistry::load(&pipelines_dir)?;
    tracing::info!("Loaded ingest pipelines from {}", pipelines_dir.display());

    // Create and start server
    let server = prism::api::ApiServer::with_pipelines(
        manager,
        config.server.cors.clone(),
        config.security.clone(),
        pipeline_registry,
    );

    let tls = if config.server.tls.enabled {
        Some(&config.server.tls)
    } else {
        None
    };

    tracing::info!("Listening on {}", addr);
    if tls.is_some() {
        tracing::info!("TLS enabled");
    }
    if config.security.enabled {
        tracing::info!(
            "Security enabled ({} API keys, {} roles)",
            config.security.api_keys.len(),
            config.security.roles.len()
        );
    }
    if config.security.audit.enabled {
        tracing::info!(
            "Audit logging enabled (index_to_collection: {})",
            config.security.audit.index_to_collection
        );
    }

    server.serve(&addr, tls).await?;

    Ok(())
}
