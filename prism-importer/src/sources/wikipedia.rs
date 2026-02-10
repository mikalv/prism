use async_trait::async_trait;
use bzip2::read::BzDecoder;
use futures::Stream;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::pin::Pin;

use super::traits::{ImportSource, SourceDocument};
use crate::error::{ImportError, Result};
use crate::schema::{SourceField, SourceFieldType, SourceSchema};

pub struct WikipediaSource {
    path: PathBuf,
}

impl WikipediaSource {
    pub fn new(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            return Err(ImportError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", path.display()),
            )));
        }
        Ok(Self { path })
    }

    fn open_reader(path: &Path) -> Result<Box<dyn BufRead + Send>> {
        let file = File::open(path)?;
        if path.extension().is_some_and(|e| e == "bz2") {
            Ok(Box::new(BufReader::with_capacity(
                256 * 1024,
                BzDecoder::new(BufReader::new(file)),
            )))
        } else {
            Ok(Box::new(BufReader::with_capacity(256 * 1024, file)))
        }
    }
}

/// Strip wikitext markup to produce plain text
fn strip_wikitext(text: &str) -> String {
    lazy_static_regexes(text)
}

fn lazy_static_regexes(text: &str) -> String {
    // Pre-compile regexes (called per article but Regex::new is cheap with caching)
    let re_comment = Regex::new(r"(?s)<!--.*?-->").unwrap();
    let re_ref = Regex::new(r"(?s)<ref[^>]*>.*?</ref>|<ref[^/]*/\s*>").unwrap();
    let re_template = Regex::new(r"\{\{[^}]*\}\}").unwrap();
    let re_table = Regex::new(r"(?s)\{\|.*?\|\}").unwrap();
    let re_file = Regex::new(r"(?i)\[\[(Fil|File|Image|Bilde):[^\]]*\]\]").unwrap();
    let re_category = Regex::new(r"(?i)\[\[(Kategori|Category):[^\]]*\]\]").unwrap();
    let re_piped_link = Regex::new(r"\[\[[^|\]]*\|([^\]]*)\]\]").unwrap();
    let re_simple_link = Regex::new(r"\[\[([^\]]*)\]\]").unwrap();
    let re_ext_link_named = Regex::new(r"\[[^ \]]+ ([^\]]*)\]").unwrap();
    let re_ext_link_bare = Regex::new(r"\[https?://[^\]]*\]").unwrap();
    let re_bold_italic = Regex::new(r"'{2,5}").unwrap();
    let re_heading = Regex::new(r"(?m)^=+\s*(.*?)\s*=+$").unwrap();
    let re_html = Regex::new(r"<[^>]+>").unwrap();
    let re_indent = Regex::new(r"(?m)^[*#:;]+\s*").unwrap();
    let re_multi_newline = Regex::new(r"\n{3,}").unwrap();

    let mut s = text.to_string();
    // Remove comments
    s = re_comment.replace_all(&s, "").to_string();
    // Remove references
    s = re_ref.replace_all(&s, "").to_string();
    // Remove templates (single-level)
    // Apply twice to handle some nesting
    s = re_template.replace_all(&s, "").to_string();
    s = re_template.replace_all(&s, "").to_string();
    // Remove tables
    s = re_table.replace_all(&s, "").to_string();
    // Remove file/image links (Norwegian: Fil/Bilde)
    s = re_file.replace_all(&s, "").to_string();
    // Remove category links
    s = re_category.replace_all(&s, "").to_string();
    // Convert piped links [[target|text]] → text
    s = re_piped_link.replace_all(&s, "$1").to_string();
    // Convert simple links [[text]] → text
    s = re_simple_link.replace_all(&s, "$1").to_string();
    // External links [url text] → text
    s = re_ext_link_named.replace_all(&s, "$1").to_string();
    // Bare external links
    s = re_ext_link_bare.replace_all(&s, "").to_string();
    // Bold/italic markers
    s = re_bold_italic.replace_all(&s, "").to_string();
    // Headings == Title == → Title
    s = re_heading.replace_all(&s, "$1").to_string();
    // Strip remaining HTML
    s = re_html.replace_all(&s, "").to_string();
    // List/indent markers
    s = re_indent.replace_all(&s, "").to_string();
    // Collapse excessive newlines
    s = re_multi_newline.replace_all(&s, "\n\n").to_string();

    s.trim().to_string()
}

/// Parse pages from a Wikipedia XML dump.
/// Returns (title, id, timestamp, text) for main namespace non-redirect articles.
fn parse_pages(
    reader: Box<dyn BufRead + Send>,
) -> impl Iterator<Item = Result<(String, String, String, String)>> + Send {
    PageIterator::new(reader)
}

struct PageIterator {
    reader: Reader<Box<dyn BufRead + Send>>,
    buf: Vec<u8>,
    finished: bool,
}

impl PageIterator {
    fn new(reader: Box<dyn BufRead + Send>) -> Self {
        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(true);
        Self {
            reader: xml_reader,
            buf: Vec::with_capacity(8 * 1024),
            finished: false,
        }
    }
}

impl Iterator for PageIterator {
    type Item = Result<(String, String, String, String)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        loop {
            self.buf.clear();
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(ref e)) if e.name().as_ref() == b"page" => {
                    match parse_single_page(&mut self.reader) {
                        Ok(Some(page)) => return Some(Ok(page)),
                        Ok(None) => continue, // redirect or non-main namespace
                        Err(e) => {
                            tracing::warn!("Error parsing page: {}", e);
                            continue;
                        }
                    }
                }
                Ok(Event::Eof) => {
                    self.finished = true;
                    return None;
                }
                Err(e) => {
                    self.finished = true;
                    return Some(Err(ImportError::XmlParse(format!(
                        "XML parse error: {}",
                        e
                    ))));
                }
                _ => {}
            }
        }
    }
}

/// Parse a single <page> element, returning None for redirects or non-main namespace
fn parse_single_page(
    reader: &mut Reader<Box<dyn BufRead + Send>>,
) -> Result<Option<(String, String, String, String)>> {
    let mut buf = Vec::with_capacity(4096);
    let mut title = String::new();
    let mut id = String::new();
    let mut ns = String::new();
    let mut timestamp = String::new();
    let mut text = String::new();
    let mut is_redirect = false;
    let mut depth = 1u32; // already inside <page>
    let mut in_revision = false;
    let mut current_tag: Option<String> = None;

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match tag_name.as_str() {
                    "revision" => in_revision = true,
                    "title" | "id" | "ns" | "timestamp" | "text" => {
                        current_tag = Some(tag_name);
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == b"redirect" {
                    is_redirect = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(ref tag) = current_tag {
                    let val = e.unescape().unwrap_or_default().to_string();
                    match tag.as_str() {
                        "title" if !in_revision => title = val,
                        "id" if !in_revision && id.is_empty() => id = val,
                        "ns" => ns = val,
                        "timestamp" if in_revision => timestamp = val,
                        "text" if in_revision => text = val,
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let tag_name = name.as_ref();
                if tag_name == b"revision" {
                    in_revision = false;
                }
                current_tag = None;
                depth -= 1;
                if depth == 0 {
                    break; // end of <page>
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ImportError::XmlParse(format!(
                    "Error inside <page>: {}",
                    e
                )));
            }
            _ => {}
        }
    }

    // Filter: namespace 0 only, skip redirects
    if ns != "0" || is_redirect {
        return Ok(None);
    }

    // Skip if no content
    if text.is_empty() {
        return Ok(None);
    }

    Ok(Some((title, id, timestamp, text)))
}

#[async_trait]
impl ImportSource for WikipediaSource {
    async fn fetch_schema(&self) -> Result<SourceSchema> {
        Ok(SourceSchema {
            name: "wikipedia".to_string(),
            fields: vec![
                SourceField {
                    name: "title".to_string(),
                    field_type: SourceFieldType::Text,
                    indexed: true,
                    vector_dims: None,
                },
                SourceField {
                    name: "content".to_string(),
                    field_type: SourceFieldType::Text,
                    indexed: true,
                    vector_dims: None,
                },
                SourceField {
                    name: "timestamp".to_string(),
                    field_type: SourceFieldType::Keyword,
                    indexed: false,
                    vector_dims: None,
                },
            ],
        })
    }

    async fn count_documents(&self) -> Result<u64> {
        // Quick estimate: scan for <page> tags
        // For bz2 this means decompressing, so we use a rough file-size estimate
        let metadata = std::fs::metadata(&self.path)?;
        let file_size = metadata.len();

        if self.path.extension().is_some_and(|e| e == "bz2") {
            // Norwegian Wikipedia: ~600k articles in ~777MB bz2
            // Rough: ~1300 bytes compressed per article
            Ok(file_size / 1300)
        } else {
            // Uncompressed: ~5KB per article average
            Ok(file_size / 5000)
        }
    }

    fn stream_documents(&self) -> Pin<Box<dyn Stream<Item = Result<SourceDocument>> + Send + '_>> {
        Box::pin(async_stream::try_stream! {
            let reader = WikipediaSource::open_reader(&self.path)?;

            // Use spawn_blocking since XML parsing is CPU-bound
            let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<SourceDocument>>(1024);

            let path = self.path.clone();
            tokio::task::spawn_blocking(move || {
                let reader = match WikipediaSource::open_reader(&path) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.blocking_send(Err(e));
                        return;
                    }
                };

                for result in parse_pages(reader) {
                    let doc = match result {
                        Ok((title, id, timestamp, raw_text)) => {
                            let content = strip_wikitext(&raw_text);
                            // Skip very short articles (stubs with < 50 chars of content)
                            if content.len() < 50 {
                                continue;
                            }
                            Ok(SourceDocument {
                                id,
                                fields: serde_json::json!({
                                    "title": title,
                                    "content": content,
                                    "timestamp": timestamp,
                                }),
                            })
                        }
                        Err(e) => Err(e),
                    };
                    if tx.blocking_send(doc).is_err() {
                        break; // receiver dropped
                    }
                }
            });

            // Drop the unused reader from the outer scope
            drop(reader);

            while let Some(doc) = rx.recv().await {
                yield doc?;
            }
        })
    }

    fn source_name(&self) -> &str {
        "wikipedia"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_wikitext_links() {
        assert_eq!(strip_wikitext("[[Norge]]"), "Norge");
        assert_eq!(strip_wikitext("[[Oslo|hovedstaden]]"), "hovedstaden");
    }

    #[test]
    fn test_strip_wikitext_templates() {
        assert_eq!(strip_wikitext("Hello {{cite web|url=x}} world"), "Hello  world");
    }

    #[test]
    fn test_strip_wikitext_bold_italic() {
        assert_eq!(strip_wikitext("'''Norge''' er et ''land''"), "Norge er et land");
    }

    #[test]
    fn test_strip_wikitext_headings() {
        assert_eq!(strip_wikitext("== Historie =="), "Historie");
    }

    #[test]
    fn test_strip_wikitext_html() {
        assert_eq!(strip_wikitext("Hello<br/>world"), "Helloworld");
        assert_eq!(strip_wikitext("<b>bold</b> text"), "bold text");
    }

    #[test]
    fn test_strip_wikitext_categories() {
        assert_eq!(strip_wikitext("Text [[Kategori:Norge]]"), "Text");
    }

    #[test]
    fn test_strip_wikitext_files() {
        assert_eq!(strip_wikitext("[[Fil:Norway.png|thumb|caption]]"), "");
    }
}
