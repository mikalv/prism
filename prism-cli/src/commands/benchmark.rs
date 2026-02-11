use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};
use tantivy::collector::{Count, TopDocs};
use tantivy::query::QueryParser;
use tantivy::schema::Field;
use tantivy::Index;

/// Statistics for a single query
#[derive(Debug)]
struct QueryStats {
    query: String,
    times: Vec<Duration>,
    hits: usize,
}

impl QueryStats {
    fn avg(&self) -> Duration {
        if self.times.is_empty() {
            return Duration::ZERO;
        }
        let total: Duration = self.times.iter().sum();
        total / self.times.len() as u32
    }

    fn p50(&self) -> Duration {
        percentile(&self.times, 50)
    }

    fn p99(&self) -> Duration {
        percentile(&self.times, 99)
    }

    fn min(&self) -> Duration {
        self.times.iter().min().copied().unwrap_or(Duration::ZERO)
    }

    fn max(&self) -> Duration {
        self.times.iter().max().copied().unwrap_or(Duration::ZERO)
    }
}

fn percentile(times: &[Duration], p: usize) -> Duration {
    if times.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted: Vec<_> = times.to_vec();
    sorted.sort();
    let idx = ((p as f64 / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn format_duration(d: Duration) -> String {
    let micros = d.as_micros();
    if micros >= 1_000_000 {
        format!("{:.2}s", d.as_secs_f64())
    } else if micros >= 1000 {
        format!("{:.2}ms", micros as f64 / 1000.0)
    } else {
        format!("{}µs", micros)
    }
}

/// Run benchmark command
pub fn run_benchmark(
    data_dir: &Path,
    collection: &str,
    queries_file: &Path,
    repeat: usize,
    warmup: usize,
    top_k: usize,
) -> Result<()> {
    let index_path = data_dir.join("collections").join(collection).join("text");

    if !index_path.exists() {
        anyhow::bail!(
            "Collection '{}' not found at {:?}. Make sure the collection exists.",
            collection,
            index_path
        );
    }

    let index = Index::open_in_dir(&index_path)
        .with_context(|| format!("Failed to open index at {:?}", index_path))?;

    let reader = index.reader()?;
    let searcher = reader.searcher();
    let schema = index.schema();

    // Get all indexed text fields for query parsing
    let search_fields: Vec<Field> = schema
        .fields()
        .filter(|(_, entry)| entry.is_indexed())
        .map(|(field, _)| field)
        .collect();

    if search_fields.is_empty() {
        anyhow::bail!("No indexed fields found in collection");
    }

    let query_parser = QueryParser::new(schema, search_fields, index.tokenizers().clone());

    // Read queries
    let queries = read_queries(queries_file)?;
    if queries.is_empty() {
        anyhow::bail!("No queries found in {:?}", queries_file);
    }

    println!(
        "Benchmark: collection '{}' ({} queries, {} repeats)",
        collection,
        queries.len(),
        repeat
    );
    println!("Index: {:?}", index_path);
    println!();

    // Warmup phase
    if warmup > 0 {
        print!("Warming up ({} iterations)...", warmup);
        for _ in 0..warmup {
            for query_str in &queries {
                if let Ok(query) = query_parser.parse_query(query_str) {
                    let _ = searcher.search(&query, &TopDocs::with_limit(top_k));
                }
            }
        }
        println!(" done");
        println!();
    }

    // Benchmark phase
    let mut all_stats: Vec<QueryStats> = queries
        .iter()
        .map(|q| QueryStats {
            query: q.clone(),
            times: Vec::with_capacity(repeat),
            hits: 0,
        })
        .collect();

    println!("Running benchmark...");

    for iteration in 0..repeat {
        for (i, query_str) in queries.iter().enumerate() {
            let query = query_parser
                .parse_query(query_str)
                .with_context(|| format!("Failed to parse query: {}", query_str))?;

            let start = Instant::now();
            let (_docs, count) = searcher.search(&query, &(TopDocs::with_limit(top_k), Count))?;
            let elapsed = start.elapsed();

            all_stats[i].times.push(elapsed);
            if iteration == 0 {
                all_stats[i].hits = count;
            }
        }
    }

    // Print results
    println!();
    println!(
        "{:<40} {:>8} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Query", "Hits", "Avg", "Min", "P50", "P99", "Max"
    );
    println!("{}", "-".repeat(100));

    for stats in &all_stats {
        let query_display = if stats.query.len() > 38 {
            format!("{}...", &stats.query[..35])
        } else {
            stats.query.clone()
        };

        println!(
            "{:<40} {:>8} {:>10} {:>10} {:>10} {:>10} {:>10}",
            query_display,
            stats.hits,
            format_duration(stats.avg()),
            format_duration(stats.min()),
            format_duration(stats.p50()),
            format_duration(stats.p99()),
            format_duration(stats.max())
        );
    }

    // Summary
    let all_times: Vec<Duration> = all_stats.iter().flat_map(|s| s.times.clone()).collect();
    let total_queries = all_times.len();
    let total_time: Duration = all_times.iter().sum();
    let avg_time = if total_queries > 0 {
        total_time / total_queries as u32
    } else {
        Duration::ZERO
    };

    println!();
    println!("Summary:");
    println!("  Total queries executed: {}", total_queries);
    println!("  Total time:             {}", format_duration(total_time));
    println!("  Average per query:      {}", format_duration(avg_time));
    println!(
        "  Queries per second:     {:.1}",
        if total_time.as_secs_f64() > 0.0 {
            total_queries as f64 / total_time.as_secs_f64()
        } else {
            0.0
        }
    );

    Ok(())
}

fn read_queries(path: &Path) -> Result<Vec<String>> {
    let file = File::open(path).with_context(|| format!("Failed to open {:?}", path))?;
    let reader = BufReader::new(file);

    let queries: Vec<String> = reader
        .lines()
        .map_while(|l| l.ok())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    Ok(queries)
}
