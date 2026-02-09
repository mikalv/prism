//! Detach command — calls the running Prism server to detach a collection.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Run the detach command via the admin API.
pub async fn run_detach(
    api_url: &str,
    collection: &str,
    output: PathBuf,
    delete_data: bool,
) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/_admin/collections/{}/detach",
        api_url.trim_end_matches('/'),
        collection
    );

    let body = serde_json::json!({
        "destination": {
            "type": "file",
            "path": output,
        },
        "delete_data": delete_data,
    });

    println!("Detaching collection '{}' ...", collection);

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to connect to Prism server")?;

    if resp.status().is_success() {
        let result: serde_json::Value = resp.json().await?;
        println!("Detached successfully.");
        println!(
            "  Snapshot: {}",
            result["destination"]["path"]
                .as_str()
                .unwrap_or("unknown")
        );
        println!(
            "  Data deleted: {}",
            result["data_deleted"].as_bool().unwrap_or(false)
        );
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Detach failed ({}): {}", status, body);
    }

    Ok(())
}
