//! Attach command — calls the running Prism server to attach a collection from a snapshot.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Run the attach command via the admin API.
pub async fn run_attach(
    api_url: &str,
    input: PathBuf,
    target_collection: Option<String>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/_admin/collections/attach",
        api_url.trim_end_matches('/')
    );

    let body = serde_json::json!({
        "source": {
            "type": "file",
            "path": input,
        },
        "target_collection": target_collection,
    });

    println!("Attaching collection from {:?} ...", input);

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to connect to Prism server")?;

    if resp.status().is_success() {
        let result: serde_json::Value = resp.json().await?;
        println!("Attached successfully.");
        println!(
            "  Collection: {}",
            result["collection"].as_str().unwrap_or("unknown")
        );
        println!(
            "  Files extracted: {}",
            result["files_extracted"].as_u64().unwrap_or(0)
        );
        println!(
            "  Bytes extracted: {}",
            result["bytes_extracted"].as_u64().unwrap_or(0)
        );
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Attach failed ({}): {}", status, body);
    }

    Ok(())
}
