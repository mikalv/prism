use anyhow::Result;

/// Show upgrade status for all cluster nodes
pub async fn run_upgrade_status(api_url: &str) -> Result<()> {
    let url = format!("{}/cluster/upgrade/status", api_url.trim_end_matches('/'));
    let resp = reqwest::get(&url).await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Request failed ({}): {}", status, body);
    }

    let body: serde_json::Value = resp.json().await?;

    println!("Cluster Upgrade Status");
    println!("======================");
    println!(
        "Total nodes: {}",
        body.get("total_nodes").and_then(|v| v.as_u64()).unwrap_or(0)
    );
    println!(
        "Draining:    {}",
        body.get("draining_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    );
    println!(
        "All same version: {}",
        body.get("all_same_version")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    );
    println!();

    if let Some(nodes) = body.get("nodes").and_then(|v| v.as_array()) {
        println!(
            "{:<20} {:<10} {:<10} {:<10} {:<10} {:<10}",
            "NODE", "VERSION", "PROTO", "MIN_PROTO", "DRAINING", "REACHABLE"
        );
        println!("{}", "-".repeat(70));
        for node in nodes {
            println!(
                "{:<20} {:<10} {:<10} {:<10} {:<10} {:<10}",
                node.get("node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?"),
                node.get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?"),
                node.get("protocol_version")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                node.get("min_supported_version")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                node.get("draining")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                node.get("reachable")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            );
        }
    }

    Ok(())
}

/// Drain a node (stop routing queries to it)
pub async fn run_drain(api_url: &str, node_id: &str) -> Result<()> {
    let url = format!(
        "{}/cluster/nodes/{}/drain",
        api_url.trim_end_matches('/'),
        node_id
    );
    let client = reqwest::Client::new();
    let resp = client.post(&url).send().await?;

    if resp.status().is_success() {
        println!("Node {} is now draining", node_id);
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to drain node {} ({}): {}", node_id, status, body);
    }

    Ok(())
}

/// Undrain a node (resume routing queries to it)
pub async fn run_undrain(api_url: &str, node_id: &str) -> Result<()> {
    let url = format!(
        "{}/cluster/nodes/{}/undrain",
        api_url.trim_end_matches('/'),
        node_id
    );
    let client = reqwest::Client::new();
    let resp = client.post(&url).send().await?;

    if resp.status().is_success() {
        println!("Node {} is no longer draining", node_id);
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "Failed to undrain node {} ({}): {}",
            node_id,
            status,
            body
        );
    }

    Ok(())
}
