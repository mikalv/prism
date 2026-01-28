use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Schema error: {0}")]
    Schema(String),

    #[error("Backend error: {0}")]
    Backend(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Tantivy error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Storage error: {0}")]
    Storage(String),
}

pub type Result<T> = std::result::Result<T, Error>;
