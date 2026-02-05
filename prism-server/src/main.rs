use anyhow::Result;
use clap::Parser;
use std::path::Path;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug, Clone)]
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

    /// Schemas directory path
    #[arg(long, default_value = "schemas")]
    schemas_dir: String,

    /// Data directory path
    #[arg(long, default_value = "data")]
    data_dir: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Load config first (tracing init depends on it)
    let config = prism::config::Config::load_or_create(std::path::Path::new(&args.config))?;
    let config_path = args.config.clone();

    // Determine log format: env var overrides config
    let log_format =
        std::env::var("LOG_FORMAT").unwrap_or_else(|_| config.observability.log_format.clone());

    // Determine log level: RUST_LOG overrides config
    let log_level =
        std::env::var("RUST_LOG").unwrap_or_else(|_| config.observability.log_level.clone());

    // Initialize tracing with configured format
    let env_filter = tracing_subscriber::EnvFilter::new(&log_level);
    let registry = tracing_subscriber::registry().with(env_filter);

    if log_format == "json" {
        registry
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        registry.with(tracing_subscriber::fmt::layer()).init();
    }

    // Initialize Prometheus metrics recorder
    let metrics_handle = if config.observability.metrics_enabled {
        let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
        let handle = builder
            .install_recorder()
            .expect("Failed to install Prometheus metrics recorder");
        tracing::info!("Prometheus metrics enabled at /metrics");
        Some(handle)
    } else {
        None
    };

    tracing::info!("Starting Prism server on {}:{}", args.host, args.port);
    tracing::info!("Schemas dir: {}", args.schemas_dir);
    tracing::info!("Data dir: {}", args.data_dir);

    config.ensure_dirs()?;
    let data_path = Path::new(&args.data_dir);
    std::fs::create_dir_all(data_path)?;
    let addr = format!("{}:{}", args.host, args.port);

    // Create backends
    let text_backend = std::sync::Arc::new(prism::backends::text::TextBackend::new(
        &config.storage.data_dir,
    )?);
    let vector_backend = std::sync::Arc::new(prism::backends::VectorBackend::new(
        &config.storage.data_dir,
    )?);

    // Set up embedding provider if enabled
    if config.embedding.enabled {
        tracing::info!("Setting up embedding provider...");
        match prism::embedding::create_provider(&config.embedding.provider).await {
            Ok(provider) => {
                let cache_path = config
                    .embedding
                    .cache_dir
                    .clone()
                    .unwrap_or_else(|| config.storage.data_dir.join("embedding_cache.db"));
                let cache = std::sync::Arc::new(
                    prism::cache::SqliteCache::new(cache_path.to_str().unwrap_or("embedding_cache.db"))
                        .expect("Failed to create embedding cache"),
                );
                let cached_provider = std::sync::Arc::new(prism::embedding::CachedEmbeddingProvider::new(
                    provider,
                    cache,
                    prism::cache::KeyStrategy::ModelText,
                ));
                vector_backend.set_embedding_provider(cached_provider);
                tracing::info!("Embedding provider configured successfully");
            }
            Err(e) => {
                tracing::warn!("Failed to create embedding provider: {}. Vector search with auto-embedding will not work.", e);
            }
        }
    }

    // Create collection manager
    // Use CLI arg for schemas_dir if provided, otherwise fall back to config
    let schemas_path = if args.schemas_dir != "schemas" {
        std::path::PathBuf::from(&args.schemas_dir)
    } else {
        config.schemas_dir()
    };
    let manager = std::sync::Arc::new(prism::collection::CollectionManager::new(
        &schemas_path,
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

    // Create server
    let server = prism::api::ApiServer::with_pipelines(
        manager,
        config.server.cors.clone(),
        config.security.clone(),
        pipeline_registry,
    )
    .with_metrics(metrics_handle);

    // Get config reloader for SIGHUP handling
    let config_reloader = server.config_reloader();

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

    // Spawn SIGHUP handler for config reload
    #[cfg(unix)]
    {
        let reloader = config_reloader.clone();
        let cfg_path = config_path.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};

            let mut sighup = signal(SignalKind::hangup()).expect("Failed to register SIGHUP handler");

            loop {
                sighup.recv().await;
                tracing::info!("Received SIGHUP, reloading configuration...");

                match prism::config::Config::load_or_create(std::path::Path::new(&cfg_path)) {
                    Ok(new_config) => {
                        reloader.reload_security(new_config.security).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to reload config: {}", e);
                    }
                }
            }
        });
        tracing::info!("SIGHUP handler registered - send SIGHUP to reload security config");
    }

    server.serve(&addr, tls).await?;

    Ok(())
}
