use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let ui_source_dir = Path::new(&manifest_dir).join("../websearch-ui");
    let dist_dir = ui_source_dir.join("dist");

    // Re-run if websearch-ui source changes
    println!("cargo:rerun-if-changed=../websearch-ui/src");
    println!("cargo:rerun-if-changed=../websearch-ui/package.json");
    println!("cargo:rerun-if-changed=../websearch-ui/vite.config.ts");
    println!("cargo:rerun-if-changed=../websearch-ui/index.html");

    // Check if websearch-ui directory exists
    if !ui_source_dir.exists() {
        println!(
            "cargo:warning=websearch-ui directory not found at {:?}. UI will not be embedded.",
            ui_source_dir
        );
        // Create empty dist directory so rust-embed doesn't fail
        std::fs::create_dir_all(&dist_dir).ok();
        std::fs::write(dist_dir.join(".gitkeep"), "").ok();
        return;
    }

    // Check if node_modules exists, run npm install if not
    let node_modules = ui_source_dir.join("node_modules");
    if !node_modules.exists() {
        println!("cargo:warning=Running npm install in websearch-ui...");

        let npm = find_npm();
        let status = Command::new(&npm)
            .arg("install")
            .current_dir(&ui_source_dir)
            .status();

        match status {
            Ok(s) if s.success() => {
                println!("cargo:warning=npm install completed successfully");
            }
            Ok(s) => {
                println!(
                    "cargo:warning=npm install failed with exit code: {:?}",
                    s.code()
                );
                return;
            }
            Err(e) => {
                println!("cargo:warning=Failed to run npm install: {}", e);
                return;
            }
        }
    }

    // Check if dist exists and has content
    let needs_build = if dist_dir.exists() {
        // Check if dist/index.html exists
        !dist_dir.join("index.html").exists()
    } else {
        true
    };

    if needs_build {
        println!("cargo:warning=Building websearch-ui with npm run build...");

        let npm = find_npm();
        let status = Command::new(&npm)
            .arg("run")
            .arg("build")
            .current_dir(&ui_source_dir)
            .status();

        match status {
            Ok(s) if s.success() => {
                println!("cargo:warning=UI build completed successfully");
            }
            Ok(s) => {
                println!(
                    "cargo:warning=UI build failed with exit code: {:?}. Creating placeholder.",
                    s.code()
                );
                create_placeholder_dist(&dist_dir);
            }
            Err(e) => {
                println!(
                    "cargo:warning=Failed to run npm build: {}. Creating placeholder.",
                    e
                );
                create_placeholder_dist(&dist_dir);
            }
        }
    }
}

fn create_placeholder_dist(dist_dir: &Path) {
    std::fs::create_dir_all(dist_dir).ok();
    let placeholder_html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Prism UI</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            min-height: 100vh;
            margin: 0;
            background: #1a1a2e;
            color: #eaeaea;
        }
        .container {
            text-align: center;
            padding: 2rem;
        }
        h1 { color: #7c3aed; }
        p { color: #a0a0a0; }
        code {
            background: #2d2d44;
            padding: 0.5rem 1rem;
            border-radius: 4px;
            display: inline-block;
            margin-top: 1rem;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>Prism UI</h1>
        <p>UI not built. Run the following to build:</p>
        <code>cd websearch-ui && npm run build</code>
    </div>
</body>
</html>"#;
    std::fs::write(dist_dir.join("index.html"), placeholder_html).ok();
}

fn find_npm() -> String {
    // Try to find npm in PATH
    if let Ok(path) = which::which("npm") {
        return path.to_string_lossy().to_string();
    }

    // Common locations
    let common_paths = [
        "/usr/local/bin/npm",
        "/opt/homebrew/bin/npm",
        "/usr/bin/npm",
    ];

    for path in common_paths {
        if Path::new(path).exists() {
            return path.to_string();
        }
    }

    // Fallback to just "npm" and hope it's in PATH
    "npm".to_string()
}
