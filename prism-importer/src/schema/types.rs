use serde::{Deserialize, Serialize};

/// Schema extracted from a source system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSchema {
    pub name: String,
    pub fields: Vec<SourceField>,
}

/// A field in the source schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceField {
    pub name: String,
    pub field_type: SourceFieldType,
    pub indexed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_dims: Option<usize>,
}

/// Normalized field types across sources
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SourceFieldType {
    Text,
    Keyword,
    I64,
    F64,
    Bool,
    Date,
    Vector,
    Json,
    Unknown(String),
}

impl std::fmt::Display for SourceFieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Keyword => write!(f, "keyword"),
            Self::I64 => write!(f, "i64"),
            Self::F64 => write!(f, "f64"),
            Self::Bool => write!(f, "bool"),
            Self::Date => write!(f, "date"),
            Self::Vector => write!(f, "vector"),
            Self::Json => write!(f, "json"),
            Self::Unknown(s) => write!(f, "unknown({})", s),
        }
    }
}
