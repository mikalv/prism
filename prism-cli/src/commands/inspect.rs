use anyhow::{Context, Result};
use std::path::Path;
use tantivy::schema::Schema;
use tantivy::space_usage::PerFieldSpaceUsage;
use tantivy::Index;
use tantivy_common::ByteCount;

/// Format bytes as human-readable string using tantivy's built-in formatter
fn format_bytes(bytes: ByteCount) -> String {
    bytes.human_readable()
}

/// Format raw byte count
fn format_bytes_raw(bytes: u64) -> String {
    ByteCount::from(bytes).human_readable()
}

/// Run inspect command on a collection
pub fn run_inspect(data_dir: &Path, collection: &str, verbose: bool) -> Result<()> {
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

    let schema = index.schema();
    let searcher = index.reader()?.searcher();
    let segments = searcher.segment_readers();
    let space_usage = searcher.space_usage()?;

    // Calculate totals
    let total_docs: u32 = segments.iter().map(|s| s.num_docs()).sum();
    let total_deleted: u32 = segments.iter().map(|s| s.num_deleted_docs()).sum();

    println!();
    println!("================================================================================");
    println!(
        "Collection: {} ({} segments, {} documents)",
        collection,
        segments.len(),
        total_docs
    );
    println!("================================================================================");
    println!();
    println!("Summary");
    println!("--------------------------------------------------------------------------------");
    println!("  Total documents:    {}", total_docs);
    println!(
        "  Deleted documents:  {} ({:.1}%)",
        total_deleted,
        if total_docs > 0 {
            (total_deleted as f64 / total_docs as f64) * 100.0
        } else {
            0.0
        }
    );
    println!(
        "  Total size:         {}",
        format_bytes(space_usage.total())
    );
    println!("  Index directory:    {:?}", index_path);

    // Check if merge is recommended
    if segments.len() > 3 || (total_deleted as f64 / (total_docs + total_deleted) as f64) > 0.1 {
        println!();
        println!(
            "  ⚠ Optimization recommended: run 'prism index optimize --collection {}'",
            collection
        );
    }

    println!();

    if verbose {
        // Per-segment details
        for (i, (segment_reader, segment_space_usage)) in segments
            .iter()
            .zip(space_usage.segments().iter())
            .enumerate()
        {
            println!(
                "Segment {} ({})",
                i + 1,
                segment_reader.segment_id().uuid_string()
            );
            println!(
                "--------------------------------------------------------------------------------"
            );
            println!(
                "  Documents: {} ({} deleted)",
                segment_space_usage.num_docs(),
                segment_reader.num_deleted_docs()
            );
            println!(
                "  Total size: {}",
                format_bytes(segment_space_usage.total())
            );
            println!();

            // Store usage
            println!("  Store:");
            println!(
                "    Total:   {}",
                format_bytes(segment_space_usage.store().total())
            );
            println!(
                "    Offsets: {}",
                format_bytes(segment_space_usage.store().offsets_usage())
            );
            println!();

            // Term dictionary
            print_per_field_usage(
                "  Term Dictionary:",
                &schema,
                segment_space_usage.termdict(),
            );

            // Fast fields
            print_per_field_usage("  Fast Fields:", &schema, segment_space_usage.fast_fields());

            // Postings
            print_per_field_usage("  Postings:", &schema, segment_space_usage.postings());

            // Positions
            print_per_field_usage("  Positions:", &schema, segment_space_usage.positions());

            println!();
        }
    } else {
        // Compact field-level summary
        println!("Field Space Usage");
        println!(
            "--------------------------------------------------------------------------------"
        );
        println!(
            "{:<30} {:>12} {:>12} {:>12} {:>12}",
            "Field", "TermDict", "FastField", "Postings", "Positions"
        );
        println!("{}", "-".repeat(80));

        // Aggregate across segments (store as bytes u64)
        let mut field_totals: std::collections::HashMap<String, (u64, u64, u64, u64)> =
            std::collections::HashMap::new();

        for segment_space_usage in space_usage.segments() {
            for (field, usage) in segment_space_usage.termdict().fields() {
                let name = schema.get_field_name(*field).to_string();
                let entry = field_totals.entry(name).or_insert((0, 0, 0, 0));
                entry.0 += usage.total().get_bytes();
            }
            for (field, usage) in segment_space_usage.fast_fields().fields() {
                let name = schema.get_field_name(*field).to_string();
                let entry = field_totals.entry(name).or_insert((0, 0, 0, 0));
                entry.1 += usage.total().get_bytes();
            }
            for (field, usage) in segment_space_usage.postings().fields() {
                let name = schema.get_field_name(*field).to_string();
                let entry = field_totals.entry(name).or_insert((0, 0, 0, 0));
                entry.2 += usage.total().get_bytes();
            }
            for (field, usage) in segment_space_usage.positions().fields() {
                let name = schema.get_field_name(*field).to_string();
                let entry = field_totals.entry(name).or_insert((0, 0, 0, 0));
                entry.3 += usage.total().get_bytes();
            }
        }

        let mut fields: Vec<_> = field_totals.iter().collect();
        fields.sort_by(|a, b| {
            let total_a = a.1 .0 + a.1 .1 + a.1 .2 + a.1 .3;
            let total_b = b.1 .0 + b.1 .1 + b.1 .2 + b.1 .3;
            total_b.cmp(&total_a)
        });

        for (name, (termdict, fastfield, postings, positions)) in fields {
            println!(
                "{:<30} {:>12} {:>12} {:>12} {:>12}",
                name,
                if *termdict > 0 {
                    format_bytes_raw(*termdict)
                } else {
                    "-".to_string()
                },
                if *fastfield > 0 {
                    format_bytes_raw(*fastfield)
                } else {
                    "-".to_string()
                },
                if *postings > 0 {
                    format_bytes_raw(*postings)
                } else {
                    "-".to_string()
                },
                if *positions > 0 {
                    format_bytes_raw(*positions)
                } else {
                    "-".to_string()
                }
            );
        }
        println!();
        println!("Use --verbose for detailed per-segment breakdown");
    }

    println!("--------------------------------------------------------------------------------");
    Ok(())
}

fn print_per_field_usage(title: &str, schema: &Schema, usage: &PerFieldSpaceUsage) {
    println!("{}", title);
    println!("    Total: {}", format_bytes(usage.total()));
    for (field, field_usage) in usage.fields() {
        let field_name = schema.get_field_name(*field);
        println!(
            "    - {}: {}",
            field_name,
            format_bytes(field_usage.total())
        );
    }
}
