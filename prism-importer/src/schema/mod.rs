pub mod types;
pub mod mapping;

pub use types::{SourceSchema, SourceField, SourceFieldType};
pub use mapping::convert_es_mapping;
