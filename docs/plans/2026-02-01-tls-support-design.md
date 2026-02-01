# TLS/HTTPS Support Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add HTTPS/TLS support to Prism with rustls, dual HTTP+HTTPS listening, and a cert generation script.

**Architecture:** Add `TlsConfig` to `ServerConfig`, use `tokio-rustls` to wrap a second TcpListener for HTTPS alongside the existing HTTP listener. Both share the same axum Router. Certificate generation is handled by a bundled shell script.

**Tech Stack:** rustls 0.23, tokio-rustls 0.26, rustls-pemfile 2

---

### Task 1: Add TLS dependencies

**Files:**
- Modify: `/home/meeh/prism/Cargo.toml` (workspace dependencies, lines 15-83)
- Modify: `/home/meeh/prism/prism/Cargo.toml` (crate dependencies, lines 12-74)

**Step 1: Add workspace dependencies**

In `/home/meeh/prism/Cargo.toml`, after the `tower-http` line (line 40), add:

```toml
tokio-rustls = "0.26"
rustls = { version = "0.23", default-features = false, features = ["std", "tls12", "ring"] }
rustls-pemfile = "2"
```

Note: We use `ring` instead of `aws-lc-rs` to avoid the C build dependency, keeping the pure-Rust story clean.

**Step 2: Wire to prism crate**

In `/home/meeh/prism/prism/Cargo.toml`, after the `tower-http` line (line 35), add:

```toml
tokio-rustls = { workspace = true }
rustls = { workspace = true }
rustls-pemfile = { workspace = true }
```

**Step 3: Verify it compiles**

Run: `cargo check -p prism 2>&1 | tail -5`
Expected: compiles with no errors (new deps unused but present)

**Step 4: Commit**

```bash
git add Cargo.toml prism/Cargo.toml
git commit -m "feat(tls): add rustls, tokio-rustls, rustls-pemfile dependencies"
```

---

### Task 2: Add TlsConfig to ServerConfig

**Files:**
- Modify: `/home/meeh/prism/prism/src/config/mod.rs` (lines 31-52 ServerConfig, lines 233-244 expand_paths)
- Test: `/home/meeh/prism/prism/tests/config_test.rs`

**Step 1: Write the failing test**

Add to the end of `/home/meeh/prism/prism/tests/config_test.rs`:

```rust
#[test]
fn test_default_tls_config() {
    let config = Config::default();
    assert!(!config.server.tls.enabled);
    assert_eq!(config.server.tls.bind_addr, "127.0.0.1:3443");
    assert_eq!(config.server.tls.cert_path, PathBuf::from("./conf/tls/cert.pem"));
    assert_eq!(config.server.tls.key_path, PathBuf::from("./conf/tls/key.pem"));
}

#[test]
fn test_parse_toml_with_tls() {
    let toml_content = r#"
[server]
bind_addr = "0.0.0.0:3080"

[server.tls]
enabled = true
bind_addr = "0.0.0.0:3443"
cert_path = "/etc/prism/cert.pem"
key_path = "/etc/prism/key.pem"

[storage]
data_dir = "/tmp/prism"
"#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert!(config.server.tls.enabled);
    assert_eq!(config.server.tls.bind_addr, "0.0.0.0:3443");
    assert_eq!(config.server.tls.cert_path, PathBuf::from("/etc/prism/cert.pem"));
    assert_eq!(config.server.tls.key_path, PathBuf::from("/etc/prism/key.pem"));
}

#[test]
fn test_parse_toml_without_tls_section() {
    // Existing TOML without [server.tls] should still parse with defaults
    let toml_content = r#"
[server]
bind_addr = "127.0.0.1:3080"

[storage]
data_dir = "/tmp/prism"
"#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert!(!config.server.tls.enabled);
    assert_eq!(config.server.tls.bind_addr, "127.0.0.1:3443");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p prism --test config_test test_default_tls_config 2>&1 | tail -5`
Expected: FAIL — `ServerConfig` has no field `tls`

**Step 3: Add TlsConfig struct and wire it in**

In `/home/meeh/prism/prism/src/config/mod.rs`:

After `CorsConfig` impl (after line 81), add:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_tls_bind_addr")]
    pub bind_addr: String,
    #[serde(default = "default_tls_cert_path")]
    pub cert_path: PathBuf,
    #[serde(default = "default_tls_key_path")]
    pub key_path: PathBuf,
}

fn default_tls_bind_addr() -> String {
    "127.0.0.1:3443".to_string()
}

fn default_tls_cert_path() -> PathBuf {
    PathBuf::from("./conf/tls/cert.pem")
}

fn default_tls_key_path() -> PathBuf {
    PathBuf::from("./conf/tls/key.pem")
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: default_tls_bind_addr(),
            cert_path: default_tls_cert_path(),
            key_path: default_tls_key_path(),
        }
    }
}
```

Add `tls` field to `ServerConfig` (line 37, before closing brace):

```rust
#[serde(default)]
pub tls: TlsConfig,
```

Update `ServerConfig::default()` (line 44-52) to include:

```rust
tls: TlsConfig::default(),
```

Add tilde expansion for TLS paths in `expand_paths` (after line 237):

```rust
if self.server.tls.enabled {
    self.server.tls.cert_path = expand_tilde(&self.server.tls.cert_path)?;
    self.server.tls.key_path = expand_tilde(&self.server.tls.key_path)?;
}
```

Export `TlsConfig` — add to the `pub use` or make it accessible via `config::TlsConfig`.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p prism --test config_test 2>&1 | tail -10`
Expected: All tests PASS including the 3 new ones

**Step 5: Commit**

```bash
git add prism/src/config/mod.rs prism/tests/config_test.rs
git commit -m "feat(tls): add TlsConfig to ServerConfig with serde defaults"
```

---

### Task 3: Add TLS listener to ApiServer::serve()

**Files:**
- Modify: `/home/meeh/prism/prism/src/api/server.rs` (lines 1-19 imports, lines 269-278 serve method)
- Modify: `/home/meeh/prism/prism/src/config/mod.rs` (export TlsConfig if not done)

**Step 1: Add a helper to load rustls ServerConfig**

In `/home/meeh/prism/prism/src/api/server.rs`, add these imports at the top:

```rust
use crate::config::TlsConfig;
use rustls::ServerConfig as RustlsServerConfig;
use std::io::BufReader;
use tokio_rustls::TlsAcceptor;
```

Add a private helper function before `serve()`:

```rust
fn load_rustls_config(tls: &TlsConfig) -> crate::Result<RustlsServerConfig> {
    let cert_file = std::fs::File::open(&tls.cert_path).map_err(|e| {
        crate::Error::Config(format!(
            "Cannot open TLS cert '{}': {}. Run bin/generate-cert.sh to create a self-signed certificate.",
            tls.cert_path.display(), e
        ))
    })?;
    let key_file = std::fs::File::open(&tls.key_path).map_err(|e| {
        crate::Error::Config(format!(
            "Cannot open TLS key '{}': {}. Run bin/generate-cert.sh to create a self-signed certificate.",
            tls.key_path.display(), e
        ))
    })?;

    let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| crate::Error::Config(format!("Failed to parse TLS certs: {}", e)))?;

    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
        .map_err(|e| crate::Error::Config(format!("Failed to parse TLS key: {}", e)))?
        .ok_or_else(|| crate::Error::Config("No private key found in key file".into()))?;

    let config = RustlsServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| crate::Error::Config(format!("Invalid TLS config: {}", e)))?;

    Ok(config)
}
```

**Step 2: Update `serve()` to accept TlsConfig and start dual listeners**

Replace the existing `serve` method (lines 269-278) with:

```rust
pub async fn serve(self, addr: &str, tls_config: Option<&TlsConfig>) -> Result<()> {
    let router = self.router();

    let http_listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("HTTP server listening on {}", addr);

    if let Some(tls) = tls_config.filter(|t| t.enabled) {
        let rustls_config = load_rustls_config(tls)?;
        let acceptor = TlsAcceptor::from(Arc::new(rustls_config));
        let tls_listener = tokio::net::TcpListener::bind(&tls.bind_addr).await?;
        tracing::info!("HTTPS server listening on {}", tls.bind_addr);

        let http_router = router.clone();
        let http_handle = tokio::spawn(async move {
            axum::serve(http_listener, http_router)
                .await
                .map_err(|e| crate::Error::Backend(e.to_string()))
        });

        // TLS accept loop
        let tls_handle = tokio::spawn(async move {
            loop {
                let (stream, _addr) = match tls_listener.accept().await {
                    Ok(conn) => conn,
                    Err(e) => {
                        tracing::error!("TLS accept error: {}", e);
                        continue;
                    }
                };
                let acceptor = acceptor.clone();
                let router = router.clone();
                tokio::spawn(async move {
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            let io = hyper_util::rt::TokioIo::new(tls_stream);
                            let service = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            );
                            if let Err(e) = service
                                .serve_connection(io, tower::ServiceExt::into_make_service(router))
                                .await
                            {
                                tracing::error!("TLS connection error: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::debug!("TLS handshake failed: {}", e);
                        }
                    }
                });
            }
        });

        tokio::select! {
            res = http_handle => {
                if let Ok(Err(e)) = res {
                    tracing::error!("HTTP server error: {}", e);
                }
            }
            res = tls_handle => {
                if let Ok(Err(e)) = res {
                    tracing::error!("HTTPS server error: {}", e);
                }
            }
        }
    } else {
        axum::serve(http_listener, router)
            .await
            .map_err(|e| crate::Error::Backend(e.to_string()))?;
    }

    Ok(())
}
```

**WAIT** — this approach requires `hyper-util`. Let me use a simpler pattern that stays within axum's API. Instead, we'll use `axum_server` or a simpler TLS accept loop. Actually, the simplest approach that works with axum 0.7 is:

Replace the TLS section above with a simpler loop using `axum::serve` won't work directly with TLS streams. The correct approach for axum 0.7 + tokio-rustls is a manual accept loop. But we need `hyper` and `hyper-util` for that.

**Revised approach — add hyper-util dependency:**

In workspace `Cargo.toml`, add:
```toml
hyper = { version = "1", features = ["server", "http1", "http2"] }
hyper-util = { version = "0.1", features = ["tokio", "server-auto", "http1", "http2"] }
```

In `prism/Cargo.toml`, add:
```toml
hyper = { workspace = true }
hyper-util = { workspace = true }
```

Then the TLS accept loop from above will work.

**Step 3: Verify it compiles**

Run: `cargo check -p prism 2>&1 | tail -5`
Expected: compiles clean

**Step 4: Commit**

```bash
git add Cargo.toml prism/Cargo.toml prism/src/api/server.rs
git commit -m "feat(tls): add HTTPS listener with tokio-rustls dual port support"
```

---

### Task 4: Wire TLS config through prism-server main

**Files:**
- Modify: `/home/meeh/prism/prism-server/src/main.rs` (lines 23-46)

**Step 1: Update main.rs to load config and pass TLS config**

Replace the current `main()` body (lines 23-46) with a working implementation that loads the config file, creates a `CollectionManager`, and calls `serve()` with the TLS config:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,prism=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let config = prism::config::Config::load_or_create(std::path::Path::new(&args.config))?;

    let addr = format!("{}:{}", args.host, args.port);
    tracing::info!("Starting Prism server on {}", addr);

    let manager = std::sync::Arc::new(
        prism::collection::CollectionManager::new(config.clone())?
    );

    let server = prism::api::ApiServer::with_cors(manager, config.server.cors.clone());

    let tls_config = if config.server.tls.enabled {
        Some(&config.server.tls)
    } else {
        None
    };

    server.serve(&addr, tls_config).await?;

    Ok(())
}
```

Note: The `CollectionManager::new` call may need adjustment depending on its actual constructor signature. Check the actual API and adjust accordingly.

**Step 2: Verify it compiles**

Run: `cargo check -p prism-server 2>&1 | tail -5`
Expected: compiles (may need to adjust `CollectionManager::new` args)

**Step 3: Commit**

```bash
git add prism-server/src/main.rs
git commit -m "feat(tls): wire TLS config through prism-server startup"
```

---

### Task 5: Create generate-cert.sh

**Files:**
- Create: `/home/meeh/prism/bin/generate-cert.sh`

**Step 1: Write the script**

Create `/home/meeh/prism/bin/generate-cert.sh`:

```bash
#!/usr/bin/env bash
# Generate a self-signed TLS certificate for Prism development/testing.
# Usage: bin/generate-cert.sh [output-dir]
#
# Requires: openssl
set -euo pipefail

CERT_DIR="${1:-./conf/tls}"

if ! command -v openssl &>/dev/null; then
    echo "Error: openssl is required but not found in PATH" >&2
    exit 1
fi

mkdir -p "$CERT_DIR"

openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout "$CERT_DIR/key.pem" \
    -out "$CERT_DIR/cert.pem" \
    -days 365 \
    -subj "/CN=localhost/O=Prism Dev" \
    2>/dev/null

echo "Generated self-signed certificate in $CERT_DIR"
echo "  cert: $CERT_DIR/cert.pem"
echo "  key:  $CERT_DIR/key.pem"
echo ""
echo "To enable TLS, set in your prism.toml:"
echo ""
echo "  [server.tls]"
echo "  enabled = true"
echo "  cert_path = \"$CERT_DIR/cert.pem\""
echo "  key_path = \"$CERT_DIR/key.pem\""
```

**Step 2: Make executable**

Run: `chmod +x /home/meeh/prism/bin/generate-cert.sh`

**Step 3: Test the script**

Run: `cd /home/meeh/prism && bin/generate-cert.sh /tmp/prism-tls-test && ls -la /tmp/prism-tls-test/`
Expected: cert.pem and key.pem created

**Step 4: Commit**

```bash
git add bin/generate-cert.sh
git commit -m "feat(tls): add generate-cert.sh for self-signed certificate creation"
```

---

### Task 6: Update xtask dist to include TLS artifacts

**Files:**
- Modify: `/home/meeh/prism/xtask/src/main.rs` (lines 157-217 stage fn, lines 311-329 generate_prism_toml)

**Step 1: Add conf/tls/ directory to staging**

In the `stage()` function (line 161), add `"conf/tls"` to the directory list:

```rust
for dir in &["bin", "conf/schemas", "conf/tls", "models", "data", "logs"] {
```

**Step 2: Copy generate-cert.sh to bin/**

After the `start.sh` write (line 172-180), add:

```rust
// Copy generate-cert.sh
let cert_script_src = root.join("bin/generate-cert.sh");
if cert_script_src.exists() {
    copy_file(&cert_script_src, &base.join("bin/generate-cert.sh"))?;
}
```

**Step 3: Add TLS section to generated prism.toml**

In `generate_prism_toml()` (lines 311-329), add the TLS section before the closing `"#`:

```toml
[server.tls]
# To enable HTTPS, run: bin/generate-cert.sh
# Then set enabled = true
enabled = false
bind_addr = "127.0.0.1:3443"
cert_path = "./conf/tls/cert.pem"
key_path = "./conf/tls/key.pem"
```

**Step 4: Verify xtask compiles**

Run: `cargo check -p xtask 2>&1 | tail -5`
Expected: compiles clean

**Step 5: Commit**

```bash
git add xtask/src/main.rs
git commit -m "feat(tls): include TLS config and cert script in dist bundle"
```

---

### Task 7: Integration test — manual verification

**Step 1: Generate a test cert**

Run: `cd /home/meeh/prism && bin/generate-cert.sh`

**Step 2: Create a test config with TLS enabled**

Write a temporary `test-tls.toml`:

```toml
[server]
bind_addr = "127.0.0.1:3080"

[server.tls]
enabled = true
bind_addr = "127.0.0.1:3443"
cert_path = "./conf/tls/cert.pem"
key_path = "./conf/tls/key.pem"

[storage]
data_dir = "/tmp/prism-tls-test-data"
```

**Step 3: Start the server and verify both ports**

Run: `cargo run -p prism-server -- --config test-tls.toml &`

Then test:
- HTTP: `curl http://127.0.0.1:3080/health`
- HTTPS: `curl -k https://127.0.0.1:3443/health`

Both should return a health response.

**Step 4: Test missing cert error**

Remove the cert and start again — should get a clear error message about running generate-cert.sh.

**Step 5: Clean up and commit**

```bash
rm -f test-tls.toml
rm -rf /tmp/prism-tls-test-data conf/tls
git add -A
git commit -m "feat(tls): HTTPS/TLS support with rustls (closes #61)"
```
