use crate::error::Result;
use reqwest::Client;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub struct DataImporter {
    input_dir: PathBuf,
    api_base_url: String,
    client: Client,
    batch_size: usize,
}

impl DataImporter {
    pub fn new(input_dir: impl AsRef<Path>, api_base_url: String) -> Self {
        Self {
            input_dir: input_dir.as_ref().to_path_buf(),
            api_base_url,
            client: Client::new(),
            batch_size: 100,
        }
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    pub async fn read_jsonl_file(
        &self,
        path: &Path,
    ) -> Result<Vec<serde_json::Map<String, Value>>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut documents = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let doc: serde_json::Map<String, Value> = serde_json::from_str(&line)?;
            documents.push(doc);
        }

        Ok(documents)
    }

    pub async fn import_collection(&self, collection: &str) -> Result<usize> {
        let jsonl_path = self.input_dir.join(format!("{}.jsonl", collection));

        if !jsonl_path.exists() {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("JSONL file not found: {}", jsonl_path.display()),
            )));
        }

        let documents = self.read_jsonl_file(&jsonl_path).await?;
        let total = documents.len();

        // Import in batches
        for chunk in documents.chunks(self.batch_size) {
            let url = format!("{}/collections/{}/documents", self.api_base_url, collection);

            // Wrap documents array in {"documents": [...]} as expected by IndexRequest
            let request_body = serde_json::json!({
                "documents": chunk.iter().map(|doc| {
                    // Convert Map to Document format: {id: String, fields: Map}
                    let id = doc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let mut fields = doc.clone();
                    fields.remove("id"); // Remove id from fields
                    serde_json::json!({
                        "id": id,
                        "fields": fields
                    })
                }).collect::<Vec<_>>()
            });

            let response = self.client.post(&url).json(&request_body).send().await?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_default();
                return Err(crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("HTTP error {}: {}", status, error_text),
                )));
            }
        }

        Ok(total)
    }

    pub async fn import_all(
        &self,
        collections: &[String],
    ) -> Result<std::collections::HashMap<String, usize>> {
        let mut results = std::collections::HashMap::new();

        for collection in collections {
            println!("Importing collection: {}", collection);
            match self.import_collection(collection).await {
                Ok(count) => {
                    println!("  ✓ Imported {} documents", count);
                    results.insert(collection.clone(), count);
                }
                Err(e) => {
                    eprintln!("  ✗ Failed to import {}: {}", collection, e);
                    return Err(e);
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_importer() {
        let importer = DataImporter::new("/tmp", "http://localhost:8080".to_string());
        assert_eq!(importer.batch_size, 100);
    }

    #[tokio::test]
    async fn test_with_batch_size() {
        let importer =
            DataImporter::new("/tmp", "http://localhost:8080".to_string()).with_batch_size(50);
        assert_eq!(importer.batch_size, 50);
    }
}
