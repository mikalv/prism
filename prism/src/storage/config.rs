use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageConfig {
    Local(LocalConfig),
    S3(S3Config),
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig::Local(LocalConfig::default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalConfig {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    pub bucket: String,
    #[serde(default)]
    pub prefix: Option<String>,
    pub region: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub force_path_style: bool,
    #[serde(default)]
    pub cache_dir: Option<String>,
}

impl StorageConfig {
    pub fn is_local(&self) -> bool {
        matches!(self, StorageConfig::Local(_))
    }

    pub fn is_s3(&self) -> bool {
        matches!(self, StorageConfig::S3(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_config_default() {
        let json = r#"{"type": "local"}"#;
        let config: StorageConfig = serde_json::from_str(json).unwrap();
        assert!(config.is_local());
    }

    #[test]
    fn test_s3_config_parsing() {
        let json = r#"{
            "type": "s3",
            "bucket": "my-bucket",
            "region": "us-east-1",
            "prefix": "indexes/",
            "endpoint": "http://localhost:9000",
            "force_path_style": true
        }"#;
        let config: StorageConfig = serde_json::from_str(json).unwrap();
        assert!(config.is_s3());
        if let StorageConfig::S3(s3) = config {
            assert_eq!(s3.bucket, "my-bucket");
            assert_eq!(s3.endpoint, Some("http://localhost:9000".to_string()));
            assert!(s3.force_path_style);
        }
    }

    #[test]
    fn test_default_is_local() {
        let config = StorageConfig::default();
        assert!(config.is_local());
    }
}
