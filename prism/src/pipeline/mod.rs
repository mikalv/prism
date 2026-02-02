pub mod processors;

use crate::backends::Document;
use crate::Result;

/// A processor transforms a document in-place before indexing.
pub trait Processor: Send + Sync {
    fn name(&self) -> &str;
    fn process(&self, doc: &mut Document) -> Result<()>;
}
