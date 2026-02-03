//! Tests for config module

use prism::config::{expand_tilde, Config};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_default_config() {
    let config = Config::default();

    assert_eq!(config.server.bind_addr, "127.0.0.1:8080");
    assert!(config.server.unix_socket.is_none());
    assert_eq!(config.storage.max_local_gb, 5.0);
    assert!(config.embedding.enabled);
    assert_eq!(config.embedding.model, "all-MiniLM-L6-v2");
    assert_eq!(config.logging.level, "info");
}

#[test]
fn test_expand_tilde() {
    let home = dirs::home_dir().unwrap();

    // ~/foo -> /home/user/foo
    let expanded = expand_tilde(&PathBuf::from("~/foo")).unwrap();
    assert_eq!(expanded, home.join("foo"));

    // Just ~ -> /home/user
    let expanded = expand_tilde(&PathBuf::from("~")).unwrap();
    assert_eq!(expanded, home);

    // /absolute/path stays as is
    let expanded = expand_tilde(&PathBuf::from("/absolute/path")).unwrap();
    assert_eq!(expanded, PathBuf::from("/absolute/path"));

    // relative/path stays as is
    let expanded = expand_tilde(&PathBuf::from("relative/path")).unwrap();
    assert_eq!(expanded, PathBuf::from("relative/path"));
}

#[test]
fn test_load_from_dir() {
    let temp = tempdir().unwrap();
    let config = Config::load_from(temp.path()).unwrap();

    // data_dir should be set to the temp dir
    assert_eq!(config.storage.data_dir, temp.path());
}

#[test]
fn test_save_and_load() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("config.toml");

    let mut config = Config::default();
    config.server.bind_addr = "0.0.0.0:9999".to_string();
    config.storage.max_local_gb = 10.0;
    config.embedding.model = "custom-model".to_string();
    config.logging.level = "debug".to_string();

    config.save(&config_path).unwrap();

    let loaded = Config::load_or_create(&config_path).unwrap();
    assert_eq!(loaded.server.bind_addr, "0.0.0.0:9999");
    assert_eq!(loaded.storage.max_local_gb, 10.0);
    assert_eq!(loaded.embedding.model, "custom-model");
    assert_eq!(loaded.logging.level, "debug");
}

#[test]
fn test_ensure_dirs() {
    let temp = tempdir().unwrap();
    let mut config = Config::default();
    config.storage.data_dir = temp.path().to_path_buf();

    config.ensure_dirs().unwrap();

    assert!(temp.path().join("data/text").exists());
    assert!(temp.path().join("data/vector").exists());
    assert!(temp.path().join("schemas").exists());
    assert!(temp.path().join("cache/models").exists());
    assert!(temp.path().join("logs").exists());
}

#[test]
fn test_path_helpers() {
    let temp = tempdir().unwrap();
    let mut config = Config::default();
    config.storage.data_dir = temp.path().to_path_buf();

    assert_eq!(config.text_data_dir(), temp.path().join("data/text"));
    assert_eq!(config.vector_data_dir(), temp.path().join("data/vector"));
    assert_eq!(config.schemas_dir(), temp.path().join("schemas"));
    assert_eq!(config.model_cache_dir(), temp.path().join("cache/models"));
    assert_eq!(config.logs_dir(), temp.path().join("logs"));
}

#[test]
fn test_parse_toml() {
    let toml_content = r#"
[server]
bind_addr = "127.0.0.1:3179"

[storage]
data_dir = "/custom/path"
max_local_gb = 20.0

[embedding]
enabled = false
model = "other-model"

[logging]
level = "trace"
file = "/var/log/engraph.log"
"#;

    let config: Config = toml::from_str(toml_content).unwrap();

    assert_eq!(config.server.bind_addr, "127.0.0.1:3179");
    assert_eq!(config.storage.data_dir, PathBuf::from("/custom/path"));
    assert_eq!(config.storage.max_local_gb, 20.0);
    assert!(!config.embedding.enabled);
    assert_eq!(config.embedding.model, "other-model");
    assert_eq!(config.logging.level, "trace");
    assert_eq!(
        config.logging.file,
        Some(PathBuf::from("/var/log/engraph.log"))
    );
}

#[test]
fn test_default_tls_config() {
    let config = Config::default();
    assert!(!config.server.tls.enabled);
    assert_eq!(config.server.tls.bind_addr, "127.0.0.1:3443");
    assert_eq!(
        config.server.tls.cert_path,
        PathBuf::from("./conf/tls/cert.pem")
    );
    assert_eq!(
        config.server.tls.key_path,
        PathBuf::from("./conf/tls/key.pem")
    );
}

#[test]
fn test_parse_toml_with_tls() {
    let toml_content = r#"
[server]
bind_addr = "0.0.0.0:3080"

[server.tls]
enabled = true
bind_addr = "0.0.0.0:3443"
cert_path = "/etc/prism/cert.pem"
key_path = "/etc/prism/key.pem"

[storage]
data_dir = "/tmp/prism"
"#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert!(config.server.tls.enabled);
    assert_eq!(config.server.tls.bind_addr, "0.0.0.0:3443");
    assert_eq!(
        config.server.tls.cert_path,
        PathBuf::from("/etc/prism/cert.pem")
    );
    assert_eq!(
        config.server.tls.key_path,
        PathBuf::from("/etc/prism/key.pem")
    );
}

#[test]
fn test_parse_toml_without_tls_section() {
    let toml_content = r#"
[server]
bind_addr = "127.0.0.1:3080"

[storage]
data_dir = "/tmp/prism"
"#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert!(!config.server.tls.enabled);
    assert_eq!(config.server.tls.bind_addr, "127.0.0.1:3443");
}

#[test]
fn test_default_security_config() {
    let config = Config::default();
    assert!(!config.security.enabled);
    assert!(config.security.api_keys.is_empty());
    assert!(config.security.roles.is_empty());
    assert!(!config.security.audit.enabled);
    assert!(config.security.audit.index_to_collection);
}

#[test]
fn test_parse_security_config() {
    let toml_content = r#"
[security]
enabled = true

[[security.api_keys]]
key = "prism_ak_test123"
name = "test-service"
roles = ["analyst"]

[security.roles.analyst]
collections = { "logs-*" = ["read", "search"] }

[security.roles.admin]
collections = { "*" = ["*"] }

[security.audit]
enabled = true
index_to_collection = false
"#;

    let config: Config = toml::from_str(toml_content).unwrap();
    assert!(config.security.enabled);
    assert_eq!(config.security.api_keys.len(), 1);
    assert_eq!(config.security.api_keys[0].name, "test-service");
    assert_eq!(config.security.api_keys[0].roles, vec!["analyst"]);
    assert_eq!(config.security.roles.len(), 2);
    assert!(config.security.roles.contains_key("analyst"));
    assert!(config.security.roles.contains_key("admin"));
    assert!(config.security.audit.enabled);
    assert!(!config.security.audit.index_to_collection);
}

#[test]
fn test_parse_config_without_security_section() {
    let toml_content = r#"
[server]
bind_addr = "127.0.0.1:3080"
"#;
    let config: Config = toml::from_str(toml_content).unwrap();
    assert!(!config.security.enabled);
    assert!(config.security.api_keys.is_empty());
}

#[test]
fn test_observability_config_defaults() {
    let config = Config::default();
    assert_eq!(config.observability.log_format, "pretty");
    assert_eq!(config.observability.log_level, "info,prism=debug");
    assert!(config.observability.metrics_enabled);
}

#[test]
fn test_observability_config_from_toml() {
    let toml_str = r#"
[observability]
log_format = "json"
log_level = "debug"
metrics_enabled = false
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.observability.log_format, "json");
    assert_eq!(config.observability.log_level, "debug");
    assert!(!config.observability.metrics_enabled);
}
