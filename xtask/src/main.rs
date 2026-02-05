use std::env;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
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

#[derive(Clone, Copy, Debug, ValueEnum, Default, PartialEq, Eq)]
enum ArchiveFormat {
    #[default]
    TarGz,
    Zip,
}

impl std::fmt::Display for ArchiveFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveFormat::TarGz => write!(f, "tar.gz"),
            ArchiveFormat::Zip => write!(f, "zip"),
        }
    }
}

impl ArchiveFormat {
    fn extension(&self) -> &'static str {
        match self {
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::Zip => "zip",
        }
    }
}

/// Common target triples for cross-compilation
/// Note: Some targets may not support all features (e.g., ONNX requires prebuilt binaries)
const COMMON_TARGETS: &[(&str, &str, ArchiveFormat)] = &[
    // Linux glibc (dynamic)
    ("x86_64-unknown-linux-gnu", "linux-x86_64", ArchiveFormat::TarGz),
    ("aarch64-unknown-linux-gnu", "linux-aarch64", ArchiveFormat::TarGz),
    // Linux musl (static) - may need --features to exclude ONNX
    ("x86_64-unknown-linux-musl", "linux-x86_64-static", ArchiveFormat::TarGz),
    ("aarch64-unknown-linux-musl", "linux-aarch64-static", ArchiveFormat::TarGz),
    // macOS
    ("x86_64-apple-darwin", "darwin-x86_64", ArchiveFormat::TarGz),
    ("aarch64-apple-darwin", "darwin-aarch64", ArchiveFormat::TarGz),
    // Windows (MSVC only - GNU toolchain lacks ONNX prebuilts)
    ("x86_64-pc-windows-msvc", "windows-x86_64", ArchiveFormat::Zip),
];

#[derive(Parser, Clone)]
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

    /// Output format (tar-gz or zip)
    #[arg(long, value_enum, default_value_t = ArchiveFormat::default())]
    format: ArchiveFormat,

    /// Cargo build target triple (e.g. x86_64-unknown-linux-gnu)
    #[arg(long)]
    target: Option<String>,

    /// Build for all common targets (linux, darwin, windows)
    #[arg(long)]
    all_targets: bool,

    /// Build static Linux binaries (musl) - no glibc dependency
    #[arg(long)]
    linux_static: bool,

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
    let text =
        fs::read_to_string(root.join("Cargo.toml")).context("reading workspace Cargo.toml")?;
    let ws: CargoWorkspace = toml::from_str(&text).context("parsing workspace Cargo.toml")?;
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

    eprintln!(
        "=> cargo build --release -p prism-server -p prism-cli --features {}",
        feature_flags.join(",")
    );

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

    // Add .exe extension for Windows targets
    let bin_name = if target.as_ref().map_or(false, |t| t.contains("windows")) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
    p.push(bin_name);
    p
}

/// Get the appropriate binary extension for a target
fn bin_extension(target: &Option<String>) -> &'static str {
    if target.as_ref().map_or(false, |t| t.contains("windows")) {
        ".exe"
    } else {
        ""
    }
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

fn stage(args: &DistArgs, root: &Path, prefix: &str, staging: &Path) -> Result<()> {
    let base = staging.join(prefix);

    // Create directory skeleton
    for dir in &[
        "bin",
        "conf/schemas",
        "conf/tls",
        "conf/pipelines",
        "models",
        "data",
        "logs",
    ] {
        fs::create_dir_all(base.join(dir))?;
    }

    // -- bin/ ---------------------------------------------------------------
    let ext = bin_extension(&args.target);
    let server_src = release_bin(root, &args.target, "prism-server");
    let cli_src = release_bin(root, &args.target, "prism");

    copy_file(&server_src, &base.join(format!("bin/prism-server{}", ext)))?;
    copy_file(&cli_src, &base.join(format!("bin/prism{}", ext)))?;

    fs::write(base.join("bin/start.sh"), generate_start_sh())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(base.join("bin/start.sh"), fs::Permissions::from_mode(0o755))?;
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
            if path
                .extension()
                .map_or(false, |e| e == "yaml" || e == "yml")
            {
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
        fs::write(base.join("models/README.md"), generate_models_readme())?;
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

    let file =
        fs::File::create(output).with_context(|| format!("creating {}", output.display()))?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut ar = tar::Builder::new(enc);

    ar.append_dir_all(prefix, staging.join(prefix))?;
    ar.into_inner()?.finish()?;

    eprintln!("=> wrote {}", output.display());
    Ok(())
}

fn create_zipfile(staging: &Path, prefix: &str, output: &Path) -> Result<()> {
    use zip::write::SimpleFileOptions;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let file =
        fs::File::create(output).with_context(|| format!("creating {}", output.display()))?;
    let mut zip = zip::ZipWriter::new(file);

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    let src_dir = staging.join(prefix);
    add_dir_to_zip(&mut zip, &src_dir, prefix, options)?;

    zip.finish()?;
    eprintln!("=> wrote {}", output.display());
    Ok(())
}

fn add_dir_to_zip<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    src: &Path,
    prefix: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let name = format!("{}/{}", prefix, entry.file_name().to_string_lossy());

        if path.is_dir() {
            zip.add_directory(&name, options)?;
            add_dir_to_zip(zip, &path, &name, options)?;
        } else {
            // Use different permissions for executables vs regular files
            let file_options = if name.contains("/bin/") {
                options.unix_permissions(0o755)
            } else {
                options.unix_permissions(0o644)
            };

            zip.start_file(&name, file_options)?;
            let mut f = fs::File::open(&path)?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
        }
    }
    Ok(())
}

fn create_archive(
    format: ArchiveFormat,
    staging: &Path,
    prefix: &str,
    output: &Path,
) -> Result<()> {
    match format {
        ArchiveFormat::TarGz => create_tarball(staging, prefix, output),
        ArchiveFormat::Zip => create_zipfile(staging, prefix, output),
    }
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
    let root = workspace_root()?;
    let version = workspace_version(&root)?;

    // Determine which targets to build
    let targets: Vec<_> = if args.all_targets {
        COMMON_TARGETS.to_vec()
    } else if args.linux_static {
        // Only static Linux targets (musl)
        COMMON_TARGETS
            .iter()
            .filter(|(t, _, _)| t.contains("musl"))
            .copied()
            .collect()
    } else {
        vec![]
    };

    if !targets.is_empty() {
        // Build for selected targets
        for (target, label, default_format) in targets {
            eprintln!("\n=> Building for {} ({})", label, target);

            let mut target_args = args.clone();
            target_args.target = Some(target.to_string());
            // Use the specified format, or default based on target (zip for Windows)
            let format = if args.format == ArchiveFormat::TarGz {
                default_format
            } else {
                args.format
            };

            if let Err(e) = build_single_dist(&target_args, &root, &version, format, Some(label)) {
                eprintln!("   warning: failed to build for {}: {}", target, e);
                eprintln!("   (you may need to install the target: rustup target add {})", target);
            }
        }
        eprintln!("\n=> Build complete. Check dist/ directory.");
    } else {
        build_single_dist(&args, &root, &version, args.format, None)?;
    }

    Ok(())
}

fn build_single_dist(
    args: &DistArgs,
    root: &Path,
    version: &str,
    format: ArchiveFormat,
    label: Option<&str>,
) -> Result<()> {
    // Create prefix with optional target label
    let prefix = if let Some(lbl) = label {
        format!("prism-{}-{}", version, lbl)
    } else if let Some(ref target) = args.target {
        // Use short label if available, otherwise full target triple
        let lbl = COMMON_TARGETS
            .iter()
            .find(|(t, _, _)| *t == target.as_str())
            .map(|(_, l, _)| *l)
            .unwrap_or(target.as_str());
        format!("prism-{}-{}", version, lbl)
    } else {
        format!("prism-{}", version)
    };

    eprintln!("=> building distribution: {}", prefix);

    // 1. Build release binaries
    cargo_build(args, root)?;

    // 2. Stage into a temporary directory
    let staging = root.join("dist/.staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    stage(args, root, &prefix, &staging)?;

    // 3. Create archive
    let archive = root.join(format!("dist/{}.{}", prefix, format.extension()));
    create_archive(format, &staging, &prefix, &archive)?;

    // 4. Clean up staging
    fs::remove_dir_all(&staging)?;

    eprintln!("=> done: {}", archive.display());
    Ok(())
}
