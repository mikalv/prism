use anyhow::{Context, Result};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Source for documents to import
pub enum DocumentSource {
    FromFile(PathBuf),
    FromStdin,
}

impl DocumentSource {
    pub fn reader(&self) -> io::Result<Box<dyn BufRead + Send>> {
        match self {
            DocumentSource::FromFile(path) => {
                let file = File::open(path)?;
                Ok(Box::new(BufReader::new(file)))
            }
            DocumentSource::FromStdin => Ok(Box::new(BufReader::new(io::stdin()))),
        }
    }
}

/// Progress tracking for import
struct ImportProgress {
    docs_imported: AtomicUsize,
    bytes_processed: AtomicUsize,
    start_time: Instant,
}

impl ImportProgress {
    fn new() -> Self {
        Self {
            docs_imported: AtomicUsize::new(0),
            bytes_processed: AtomicUsize::new(0),
            start_time: Instant::now(),
        }
    }

    fn add(&self, docs: usize, bytes: usize) {
        self.docs_imported.fetch_add(docs, Ordering::Relaxed);
        self.bytes_processed.fetch_add(bytes, Ordering::Relaxed);
    }

    fn print_progress(&self) {
        let docs = self.docs_imported.load(Ordering::Relaxed);
        let bytes = self.bytes_processed.load(Ordering::Relaxed);
        let elapsed = self.start_time.elapsed().as_secs_f64();

        if elapsed > 0.0 {
            let docs_per_sec = docs as f64 / elapsed;
            let mb_per_sec = (bytes as f64 / 1_000_000.0) / elapsed;
            eprint!(
                "\r  Imported {} docs ({:.1} docs/s, {:.2} MB/s)    ",
                docs, docs_per_sec, mb_per_sec
            );
        }
    }

    fn finish(&self) {
        let docs = self.docs_imported.load(Ordering::Relaxed);
        let bytes = self.bytes_processed.load(Ordering::Relaxed);
        let elapsed = self.start_time.elapsed();

        eprintln!();
        println!();
        println!("Import completed:");
        println!("  Documents: {}", docs);
        println!("  Bytes:     {:.2} MB", bytes as f64 / 1_000_000.0);
        println!("  Time:      {:.2}s", elapsed.as_secs_f64());
        if elapsed.as_secs_f64() > 0.0 {
            println!(
                "  Throughput: {:.1} docs/s, {:.2} MB/s",
                docs as f64 / elapsed.as_secs_f64(),
                (bytes as f64 / 1_000_000.0) / elapsed.as_secs_f64()
            );
        }
    }
}

/// Run import command
pub async fn run_import(
    api_url: &str,
    collection: &str,
    source: DocumentSource,
    batch_size: usize,
    no_progress: bool,
) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/collections/{}/documents", api_url, collection);

    // First, verify the collection exists
    let check_url = format!("{}/collections/{}", api_url, collection);
    let resp = client
        .get(&check_url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to API at {}", api_url))?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "Collection '{}' not found. Create it first with the API.",
            collection
        );
    }

    println!("Importing to collection '{}' via {}", collection, api_url);
    println!("Batch size: {}", batch_size);
    println!();

    let progress = Arc::new(ImportProgress::new());
    let reader = source.reader()?;

    let mut batch: Vec<serde_json::Value> = Vec::with_capacity(batch_size);
    let mut batch_bytes = 0usize;
    let mut last_progress = Instant::now();

    for line_result in reader.lines() {
        let line = line_result.context("Failed to read line")?;
        if line.trim().is_empty() {
            continue;
        }

        let doc: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("Failed to parse JSON: {}", &line[..line.len().min(100)]))?;

        batch_bytes += line.len();
        batch.push(doc);

        if batch.len() >= batch_size {
            send_batch(&client, &url, &batch).await?;
            progress.add(batch.len(), batch_bytes);
            batch.clear();
            batch_bytes = 0;

            if !no_progress && last_progress.elapsed().as_millis() > 100 {
                progress.print_progress();
                last_progress = Instant::now();
            }
        }
    }

    // Send remaining documents
    if !batch.is_empty() {
        send_batch(&client, &url, &batch).await?;
        progress.add(batch.len(), batch_bytes);
    }

    if !no_progress {
        progress.finish();
    }

    Ok(())
}

async fn send_batch(
    client: &reqwest::Client,
    url: &str,
    batch: &[serde_json::Value],
) -> Result<()> {
    let resp = client
        .post(url)
        .json(&batch)
        .send()
        .await
        .context("Failed to send batch")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("API returned error {}: {}", status, body);
    }

    Ok(())
}
