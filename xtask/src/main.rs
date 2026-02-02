use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "xtask", about = "Prism build tasks")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build a distribution bundle (tar.gz)
    Dist(DistArgs),
}

#[derive(Parser)]
struct DistArgs {
    /// Cargo features passed to `prism` crate (comma-separated)
    #[arg(long, default_value = "full,storage-s3")]
    features: String,

    /// Download and bundle the ONNX embedding model (~80 MB)
    #[arg(long)]
    include_models: bool,

    /// HuggingFace model name for ONNX download
    #[arg(long, default_value = "all-MiniLM-L6-v2")]
    model: String,

    /// Output format (only tar.gz supported today)
    #[arg(long, default_value = "tar.gz")]
    format: String,

    /// Cargo build target triple (e.g. x86_64-unknown-linux-gnu)
    #[arg(long)]
    target: Option<String>,

    /// Skip `cargo build` and use existing release binaries
    #[arg(long)]
    skip_build: bool,
}

// ---------------------------------------------------------------------------
// Workspace metadata
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CargoWorkspace {
    workspace: WorkspaceMeta,
}

#[derive(Deserialize)]
struct WorkspaceMeta {
    package: WorkspacePackage,
}

#[derive(Deserialize)]
struct WorkspacePackage {
    version: String,
}

fn workspace_root() -> Result<PathBuf> {
    // xtask lives at <root>/xtask; at runtime CWD may vary, so derive from
    // the cargo manifest dir or fall back to walking up from the exe.
    if let Ok(dir) = env::var("CARGO_MANIFEST_DIR") {
        // When invoked via `cargo xtask`, CARGO_MANIFEST_DIR = <root>/xtask
        let root = PathBuf::from(dir)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        return Ok(root);
    }
    // Fallback: look for workspace Cargo.toml by walking up
    let mut dir = env::current_dir()?;
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("xtask").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not locate workspace root");
        }
    }
}

fn workspace_version(root: &Path) -> Result<String> {
    let text = fs::read_to_string(root.join("Cargo.toml"))
        .context("reading workspace Cargo.toml")?;
    let ws: CargoWorkspace = toml::from_str(&text)
        .context("parsing workspace Cargo.toml")?;
    Ok(ws.workspace.package.version)
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

fn cargo_build(args: &DistArgs, root: &Path) -> Result<()> {
    if args.skip_build {
        eprintln!("=> skipping build (--skip-build)");
        return Ok(());
    }

    let feature_flags: Vec<String> = args
        .features
        .split(',')
        .map(|f| format!("prism/{}", f.trim()))
        .collect();

    let mut cmd = Command::new("cargo");
    cmd.current_dir(root)
        .args(["build", "--release"])
        .args(["-p", "prism-server", "-p", "prism-cli"])
        .arg("--features")
        .arg(feature_flags.join(","));

    if let Some(triple) = &args.target {
        cmd.arg("--target").arg(triple);
    }

    eprintln!("=> cargo build --release -p prism-server -p prism-cli --features {}", feature_flags.join(","));

    let status = cmd.status().context("spawning cargo build")?;
    if !status.success() {
        bail!("cargo build failed (exit {})", status);
    }
    Ok(())
}

/// Resolve path to a release binary, accounting for an optional --target.
fn release_bin(root: &Path, target: &Option<String>, name: &str) -> PathBuf {
    let mut p = root.join("target");
    if let Some(t) = target {
        p.push(t);
    }
    p.push("release");
    p.push(name);
    p
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

fn stage(args: &DistArgs, root: &Path, prefix: &str, staging: &Path) -> Result<()> {
    let base = staging.join(prefix);

    // Create directory skeleton
    for dir in &["bin", "conf/schemas", "conf/tls", "conf/pipelines", "models", "data", "logs"] {
        fs::create_dir_all(base.join(dir))?;
    }

    // -- bin/ ---------------------------------------------------------------
    let server_src = release_bin(root, &args.target, "prism-server");
    let cli_src = release_bin(root, &args.target, "prism");

    copy_file(&server_src, &base.join("bin/prism-server"))?;
    copy_file(&cli_src, &base.join("bin/prism"))?;

    fs::write(base.join("bin/start.sh"), generate_start_sh())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            base.join("bin/start.sh"),
            fs::Permissions::from_mode(0o755),
        )?;
    }

    // Copy generate-cert.sh for TLS certificate generation
    let cert_script_src = root.join("bin/generate-cert.sh");
    if cert_script_src.exists() {
        copy_file(&cert_script_src, &base.join("bin/generate-cert.sh"))?;
    }

    // -- conf/ --------------------------------------------------------------
    fs::write(base.join("conf/prism.toml"), generate_prism_toml())?;

    // Copy schema examples from test-data
    let schemas_src = root.join("prism/test-data/schemas");
    if schemas_src.is_dir() {
        for entry in fs::read_dir(&schemas_src)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "yaml" || e == "yml") {
                let dest = base.join("conf/schemas").join(entry.file_name());
                fs::copy(&path, &dest)?;
            }
        }
    }
    // Always include a minimal example schema
    fs::write(
        base.join("conf/schemas/example.yaml"),
        generate_example_schema(),
    )?;

    // Example ingest pipeline
    fs::write(
        base.join("conf/pipelines/example.yaml"),
        generate_example_pipeline(),
    )?;

    // -- models/ ------------------------------------------------------------
    if args.include_models {
        download_model(&args.model, &base.join("models"))?;
    } else {
        fs::write(
            base.join("models/README.md"),
            generate_models_readme(),
        )?;
    }

    // -- top-level README ---------------------------------------------------
    fs::write(base.join("README.md"), generate_readme(prefix))?;

    Ok(())
}

fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    fs::copy(src, dst)
        .with_context(|| format!("copying {} -> {}", src.display(), dst.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dst, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Model download (blocking reqwest)
// ---------------------------------------------------------------------------

const HF_BASE: &str = "https://huggingface.co/sentence-transformers";

fn download_model(model: &str, dest: &Path) -> Result<()> {
    let files = ["model.onnx", "tokenizer.json", "config.json"];
    let model_dir = dest.join(model);
    fs::create_dir_all(&model_dir)?;

    let client = reqwest::blocking::Client::builder()
        .user_agent("prism-xtask/0.1")
        .build()?;

    for file in &files {
        let url = format!("{}/{}/resolve/main/onnx/{}", HF_BASE, model, file);
        eprintln!("=> downloading {}", url);

        let resp = client
            .get(&url)
            .send()
            .with_context(|| format!("fetching {}", url))?;

        if !resp.status().is_success() {
            bail!("HTTP {} for {}", resp.status(), url);
        }

        let bytes = resp.bytes()?;
        let dest_file = model_dir.join(file);
        fs::write(&dest_file, &bytes)
            .with_context(|| format!("writing {}", dest_file.display()))?;
        eprintln!("   wrote {} ({} bytes)", dest_file.display(), bytes.len());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Archive
// ---------------------------------------------------------------------------

fn create_tarball(staging: &Path, prefix: &str, output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = fs::File::create(output)
        .with_context(|| format!("creating {}", output.display()))?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut ar = tar::Builder::new(enc);

    ar.append_dir_all(prefix, staging.join(prefix))?;
    ar.into_inner()?.finish()?;

    eprintln!("=> wrote {}", output.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Generated content
// ---------------------------------------------------------------------------

fn generate_start_sh() -> &'static str {
    r#"#!/usr/bin/env bash
# Start Prism search server
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PRISM_HOME="$(dirname "$SCRIPT_DIR")"

export PRISM_HOME

exec "$SCRIPT_DIR/prism-server" \
    --config "$PRISM_HOME/conf/prism.toml" \
    "$@"
"#
}

fn generate_prism_toml() -> &'static str {
    r#"# Prism configuration
# See https://github.com/mikalv/prism for full documentation

[server]
bind_addr = "127.0.0.1:3080"

[storage]
data_dir = "./data"

[embedding]
enabled = true
model = "all-MiniLM-L6-v2"

[logging]
level = "info"
# file = "./logs/prism.log"

[server.tls]
# To enable HTTPS, run: bin/generate-cert.sh
# Then set enabled = true
enabled = false
bind_addr = "127.0.0.1:3443"
cert_path = "./conf/tls/cert.pem"
key_path = "./conf/tls/key.pem"

[security]
# Enable API key authentication and RBAC
# enabled = true

# [[security.api_keys]]
# key = "prism_ak_change_me_to_a_random_string"
# name = "default-admin"
# roles = ["admin"]

# [security.roles.admin]
# collections = { "*" = ["*"] }

[security.audit]
# Enable audit logging
enabled = false
index_to_collection = true
"#
}

fn generate_example_schema() -> &'static str {
    r#"# Example collection schema
# Place your schema files in this directory and reference them when creating
# collections via the API.

collection: example
backends:
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
      - name: content
        type: text
        stored: true
        indexed: true
      - name: category
        type: string
        indexed: true
      - name: created_at
        type: date
        indexed: true
  vector:
    dimension: 384
    distance: cosine

embedding_generation:
  enabled: true
  source_field: content
  target_field: embedding
"#
}

fn generate_example_pipeline() -> &'static str {
    r#"# Example ingest pipeline
# Reference via: POST /collections/{name}/documents?pipeline=normalize

name: normalize
description: Normalize text fields before indexing
processors:
  - lowercase:
      field: title
  - lowercase:
      field: content
  - set:
      field: indexed_at
      value: "{{_now}}"
"#
}

fn generate_models_readme() -> &'static str {
    r#"# Models

This directory is reserved for ONNX embedding models.

To download the default model, rebuild with:

    cargo xtask dist --include-models

Or manually download from HuggingFace:

    https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2

Place the following files here:

    models/all-MiniLM-L6-v2/
    ├── model.onnx
    ├── tokenizer.json
    └── config.json
"#
}

fn generate_readme(prefix: &str) -> String {
    format!(
        r#"# {prefix}

Prism – Hybrid search engine combining full-text and vector search for
AI/RAG applications.

## Quick start

    cd {prefix}
    bin/start.sh

The server listens on 127.0.0.1:3080 by default. Edit conf/prism.toml to
change the bind address, storage path, or embedding settings.

## Directory layout

    bin/          Server and CLI binaries, start script
    conf/         Configuration and collection schemas
    models/       ONNX embedding models (if bundled)
    data/         Runtime data (collections, indices)
    logs/         Log files (when file logging is enabled)

## CLI

    bin/prism --help

## Documentation

    https://github.com/mikalv/prism
"#
    )
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Cmd::Dist(args) => run_dist(args),
    }
}

fn run_dist(args: DistArgs) -> Result<()> {
    if args.format != "tar.gz" {
        bail!("unsupported format '{}'; only tar.gz is supported", args.format);
    }

    let root = workspace_root()?;
    let version = workspace_version(&root)?;
    let prefix = format!("prism-{}", version);

    eprintln!("=> building distribution: {}", prefix);

    // 1. Build release binaries
    cargo_build(&args, &root)?;

    // 2. Stage into a temporary directory
    let staging = root.join("dist/.staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    stage(&args, &root, &prefix, &staging)?;

    // 3. Create tarball
    let tarball = root.join(format!("dist/{}.tar.gz", prefix));
    create_tarball(&staging, &prefix, &tarball)?;

    // 4. Clean up staging
    fs::remove_dir_all(&staging)?;

    eprintln!("=> done: {}", tarball.display());
    Ok(())
}
