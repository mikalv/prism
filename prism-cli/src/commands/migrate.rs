//! Migrate command — copy a collection between Prism instances via HTTP API.
//!
//! Scrolls all documents from a source instance and indexes them into a target
//! instance. Works across different hosts. Does NOT delete from source.

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct ScrollResponse {
    documents: Vec<ScrollDoc>,
    #[allow(dead_code)]
    total: usize,
    has_more: bool,
}

#[derive(Deserialize)]
struct ScrollDoc {
    id: String,
    #[serde(default)]
    fields: serde_json::Map<String, serde_json::Value>,
}

pub async fn run_migrate(
    source_url: &str,
    target_url: &str,
    collection: &str,
    target_collection: Option<&str>,
    batch_size: usize,
) -> Result<()> {
    let target_name = target_collection.unwrap_or(collection);
    let client = reqwest::Client::new();

    // 1. Get source collection stats
    let stats_url = format!(
        "{}/collections/{}/stats",
        source_url.trim_end_matches('/'),
        collection
    );
    let stats: serde_json::Value = client
        .get(&stats_url)
        .send()
        .await
        .context("Failed to connect to source")?
        .json()
        .await
        .context("Failed to parse source stats")?;

    let total = stats["document_count"].as_u64().unwrap_or(0) as usize;
    println!(
        "Migrating '{}' ({} docs) from {} → {} as '{}'",
        collection, total, source_url, target_url, target_name
    );

    if total == 0 {
        println!("No documents to migrate.");
        return Ok(());
    }

    // 2. Get raw schema from source and create collection on target
    let schema_url = format!(
        "{}/collections/{}/schema/raw",
        source_url.trim_end_matches('/'),
        collection
    );
    let schema_resp = client.get(&schema_url).send().await;
    match schema_resp {
        Ok(resp) if resp.status().is_success() => {
            let schema: serde_json::Value = resp.json().await.unwrap_or_default();
            let create_url = format!(
                "{}/collections/{}",
                target_url.trim_end_matches('/'),
                target_name
            );
            let create_resp = client.put(&create_url).json(&schema).send().await;
            match create_resp {
                Ok(r) if r.status().is_success() => {
                    println!("Created collection '{}' on target", target_name);
                }
                Ok(r) if r.status().as_u16() == 409 => {
                    println!("Collection '{}' already exists on target", target_name);
                }
                Ok(r) => {
                    let body = r.text().await.unwrap_or_default();
                    println!("Warning: could not create collection on target: {}", body);
                }
                Err(e) => {
                    println!("Warning: could not create collection on target: {}", e);
                }
            }
        }
        _ => {
            println!("Warning: could not fetch schema from source, target collection must exist");
        }
    }

    // 3. Scroll and index in batches
    let mut offset = 0;
    let mut migrated = 0;

    loop {
        let scroll_url = format!(
            "{}/collections/{}/documents?offset={}&limit={}",
            source_url.trim_end_matches('/'),
            collection,
            offset,
            batch_size
        );

        let scroll: ScrollResponse = client
            .get(&scroll_url)
            .send()
            .await
            .with_context(|| format!("Failed to scroll at offset {}", offset))?
            .json()
            .await
            .with_context(|| format!("Failed to parse scroll response at offset {}", offset))?;

        let batch_count = scroll.documents.len();
        if batch_count == 0 {
            break;
        }

        // Convert scroll results to Document format: {id, fields: {...}}
        let docs: Vec<serde_json::Value> = scroll
            .documents
            .into_iter()
            .map(|doc| {
                serde_json::json!({
                    "id": doc.id,
                    "fields": doc.fields,
                })
            })
            .collect();

        // POST to target
        let index_url = format!(
            "{}/collections/{}/documents?sync=true",
            target_url.trim_end_matches('/'),
            target_name
        );

        let body = serde_json::json!({ "documents": docs });
        let resp = client
            .post(&index_url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to index batch at offset {}", offset))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Index failed at offset {} ({}): {}", offset, status, body);
        }

        migrated += batch_count;
        println!("  {}/{} documents migrated", migrated, total);

        if !scroll.has_more {
            break;
        }
        offset += batch_count;
    }

    println!(
        "Migration complete: {} documents copied to '{}'",
        migrated, target_name
    );
    Ok(())
}
