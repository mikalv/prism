use crate::Result;
use base64::Engine;
use serde_json::json;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use tantivy::Index;

pub struct DataExporter {
    old_engraph_path: PathBuf,
}

impl DataExporter {
    pub fn new(old_engraph_path: impl AsRef<Path>) -> Self {
        Self {
            old_engraph_path: old_engraph_path.as_ref().to_path_buf(),
        }
    }

    pub fn export_collection(
        &self,
        collection: &str,
        output_path: impl AsRef<Path>,
    ) -> Result<usize> {
        let index_path = self.old_engraph_path.join(collection);
        let index = Index::open_in_dir(&index_path)?;
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let schema = index.schema();

        let file = File::create(&output_path)?;
        let mut writer = BufWriter::new(file);

        let mut count = 0;

        // Iterate all documents
        for (segment_ord, segment_reader) in searcher.segment_readers().iter().enumerate() {
            let alive_bitset = segment_reader.alive_bitset();

            for doc_id in 0..segment_reader.num_docs() {
                if let Some(ref bitset) = alive_bitset {
                    if !bitset.is_alive(doc_id) {
                        continue;
                    }
                }

                let doc_address = tantivy::DocAddress::new(segment_ord as u32, doc_id);
                let doc: tantivy::TantivyDocument = searcher.doc(doc_address)?;

                // Convert to JSON
                let mut json_doc = serde_json::Map::new();

                for (_field, field_entry) in schema.fields() {
                    let field_name = field_entry.name();

                    if let Some(value) = doc.get_first(_field) {
                        let json_value = match value {
                            tantivy::schema::OwnedValue::Str(s) => json!(s),
                            tantivy::schema::OwnedValue::U64(n) => json!(n),
                            tantivy::schema::OwnedValue::I64(n) => json!(n),
                            tantivy::schema::OwnedValue::F64(n) => json!(n),
                            tantivy::schema::OwnedValue::Bool(b) => json!(b),
                            tantivy::schema::OwnedValue::Date(d) => json!(d.into_timestamp_secs()),
                            tantivy::schema::OwnedValue::Bytes(b) => {
                                json!(base64::engine::general_purpose::STANDARD.encode(b))
                            }
                            _ => continue,
                        };

                        json_doc.insert(field_name.to_string(), json_value);
                    }
                }

                // Write as JSONL
                serde_json::to_writer(&mut writer, &json_doc)?;
                writeln!(writer)?;

                count += 1;
            }
        }

        writer.flush()?;

        Ok(count)
    }

    pub fn export_all(&self, collections: &[String], output_dir: impl AsRef<Path>) -> Result<()> {
        std::fs::create_dir_all(&output_dir)?;

        for collection in collections {
            let output_file = output_dir.as_ref().join(format!("{}.jsonl", collection));
            println!("Exporting {} to {}", collection, output_file.display());

            let count = self.export_collection(collection, &output_file)?;
            println!("  Exported {} documents", count);
        }

        Ok(())
    }
}
