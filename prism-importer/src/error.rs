use thiserror::Error;

#[derive(Error, Debug)]
pub enum ImportError {
    #[error("Connection failed: {0}")]
    Connection(#[from] reqwest::Error),

    #[error("Authentication failed (status {status})")]
    Auth { status: u16 },

    #[error("Index not found: {0}")]
    IndexNotFound(String),

    #[error("Schema conversion failed for field '{field}': {reason}")]
    SchemaConversion { field: String, reason: String },

    #[error("Document error at '{id}': {reason}")]
    Document { id: String, reason: String },

    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("XML parse error: {0}")]
    XmlParse(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ImportError>;
