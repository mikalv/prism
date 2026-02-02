use crate::backends::Document;
use crate::error::Error;
use crate::Result;
use super::Processor;

/// Convert a string field to lowercase.
pub struct LowercaseProcessor {
    pub field: String,
}

impl Processor for LowercaseProcessor {
    fn name(&self) -> &str { "lowercase" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        let val = doc.fields.get(&self.field)
            .ok_or_else(|| Error::Backend(format!("lowercase: field '{}' not found", self.field)))?;
        let s = val.as_str()
            .ok_or_else(|| Error::Backend(format!("lowercase: field '{}' is not a string", self.field)))?;
        let lowered = s.to_lowercase();
        doc.fields.insert(self.field.clone(), serde_json::Value::String(lowered));
        Ok(())
    }
}

/// Strip HTML tags from a string field.
pub struct HtmlStripProcessor {
    pub field: String,
}

impl Processor for HtmlStripProcessor {
    fn name(&self) -> &str { "html_strip" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        let val = doc.fields.get(&self.field)
            .ok_or_else(|| Error::Backend(format!("html_strip: field '{}' not found", self.field)))?;
        let s = val.as_str()
            .ok_or_else(|| Error::Backend(format!("html_strip: field '{}' is not a string", self.field)))?;
        let stripped = strip_html(s);
        doc.fields.insert(self.field.clone(), serde_json::Value::String(stripped));
        Ok(())
    }
}

/// Set a field to a static value. Supports `{{_now}}` for current ISO8601 timestamp.
pub struct SetProcessor {
    pub field: String,
    pub value: String,
}

impl Processor for SetProcessor {
    fn name(&self) -> &str { "set" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        let resolved = if self.value == "{{_now}}" {
            chrono::Utc::now().to_rfc3339()
        } else {
            self.value.clone()
        };
        doc.fields.insert(self.field.clone(), serde_json::Value::String(resolved));
        Ok(())
    }
}

/// Remove a field from the document. No-op if field doesn't exist.
pub struct RemoveProcessor {
    pub field: String,
}

impl Processor for RemoveProcessor {
    fn name(&self) -> &str { "remove" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        doc.fields.remove(&self.field);
        Ok(())
    }
}

/// Rename a field.
pub struct RenameProcessor {
    pub from: String,
    pub to: String,
}

impl Processor for RenameProcessor {
    fn name(&self) -> &str { "rename" }

    fn process(&self, doc: &mut Document) -> Result<()> {
        let val = doc.fields.remove(&self.from)
            .ok_or_else(|| Error::Backend(format!("rename: field '{}' not found", self.from)))?;
        doc.fields.insert(self.to.clone(), val);
        Ok(())
    }
}

/// Simple HTML tag stripping using a state machine.
fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}
