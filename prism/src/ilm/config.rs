//! ILM configuration parsing
//!
//! Parses ILM configuration from TOML config files.

use super::types::{IlmPolicy, Phase, PhaseConfig, RolloverConditions, StorageTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Main ILM configuration section
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IlmConfig {
    /// Enable ILM processing
    #[serde(default)]
    pub enabled: bool,

    /// Interval in seconds between ILM checks
    #[serde(default = "default_check_interval")]
    pub check_interval_secs: u64,

    /// ILM policies
    #[serde(default)]
    pub policies: HashMap<String, IlmPolicyConfig>,
}

fn default_check_interval() -> u64 {
    60
}

impl IlmConfig {
    /// Convert config policies to IlmPolicy instances
    pub fn build_policies(&self) -> HashMap<String, IlmPolicy> {
        self.policies
            .iter()
            .map(|(name, config)| {
                let policy = config.to_policy(name.clone());
                (name.clone(), policy)
            })
            .collect()
    }
}

/// Configuration for a single ILM policy
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IlmPolicyConfig {
    /// Description of the policy
    #[serde(default)]
    pub description: String,

    /// Rollover when index reaches this size (e.g., "50GB", "1TB")
    #[serde(default)]
    pub rollover_max_size: Option<String>,

    /// Rollover when index reaches this age (e.g., "1d", "7d", "30d")
    #[serde(default)]
    pub rollover_max_age: Option<String>,

    /// Rollover when index reaches this document count
    #[serde(default)]
    pub rollover_max_docs: Option<usize>,

    /// Phase configurations
    #[serde(default)]
    pub phases: IlmPhasesConfig,
}

impl IlmPolicyConfig {
    /// Convert to IlmPolicy
    pub fn to_policy(&self, name: String) -> IlmPolicy {
        let rollover = RolloverConditions {
            max_size: self.rollover_max_size.as_ref().and_then(|s| parse_size(s)),
            max_age: self
                .rollover_max_age
                .as_ref()
                .and_then(|s| parse_duration(s)),
            max_docs: self.rollover_max_docs,
        };

        let mut phases = HashMap::new();

        if let Some(ref hot) = self.phases.hot {
            phases.insert(Phase::Hot, hot.to_phase_config());
        }
        if let Some(ref warm) = self.phases.warm {
            phases.insert(Phase::Warm, warm.to_phase_config());
        }
        if let Some(ref cold) = self.phases.cold {
            phases.insert(Phase::Cold, cold.to_phase_config());
        }
        if let Some(ref frozen) = self.phases.frozen {
            phases.insert(Phase::Frozen, frozen.to_phase_config());
        }
        if let Some(ref delete) = self.phases.delete {
            phases.insert(Phase::Delete, delete.to_phase_config());
        }

        IlmPolicy {
            name,
            description: self.description.clone(),
            rollover,
            phases,
        }
    }
}

/// Phase configurations container
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IlmPhasesConfig {
    pub hot: Option<PhaseConfigEntry>,
    pub warm: Option<PhaseConfigEntry>,
    pub cold: Option<PhaseConfigEntry>,
    pub frozen: Option<PhaseConfigEntry>,
    pub delete: Option<PhaseConfigEntry>,
}

/// Configuration for a single phase
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhaseConfigEntry {
    /// Minimum age before entering this phase (e.g., "0d", "7d", "30d")
    #[serde(default = "default_min_age")]
    pub min_age: String,

    /// Make index read-only in this phase
    #[serde(default)]
    pub readonly: bool,

    /// Storage tier ("local" or "s3")
    #[serde(default)]
    pub storage: Option<String>,

    /// Force merge to this number of segments
    #[serde(default)]
    pub force_merge_segments: Option<usize>,

    /// Shrink to this number of shards (future)
    #[serde(default)]
    pub shrink_shards: Option<usize>,
}

fn default_min_age() -> String {
    "0d".to_string()
}

impl PhaseConfigEntry {
    /// Convert to PhaseConfig
    pub fn to_phase_config(&self) -> PhaseConfig {
        PhaseConfig {
            min_age: parse_duration(&self.min_age).unwrap_or_default(),
            readonly: self.readonly,
            storage: self
                .storage
                .as_ref()
                .and_then(|s| match s.to_lowercase().as_str() {
                    "local" => Some(StorageTier::Local),
                    "s3" => Some(StorageTier::S3),
                    _ => None,
                }),
            force_merge_segments: self.force_merge_segments,
            shrink_shards: self.shrink_shards,
        }
    }
}

/// Parse a size string like "50GB", "1TB", "500MB" to bytes
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();

    // Try to find the unit suffix
    let (num_str, multiplier) = if s.ends_with("TB") {
        (&s[..s.len() - 2], 1024u64 * 1024 * 1024 * 1024)
    } else if s.ends_with("GB") {
        (&s[..s.len() - 2], 1024u64 * 1024 * 1024)
    } else if s.ends_with("MB") {
        (&s[..s.len() - 2], 1024u64 * 1024)
    } else if s.ends_with("KB") {
        (&s[..s.len() - 2], 1024u64)
    } else if s.ends_with("B") {
        (&s[..s.len() - 1], 1u64)
    } else {
        // Assume bytes if no unit
        (s.as_str(), 1u64)
    };

    num_str
        .trim()
        .parse::<f64>()
        .ok()
        .map(|n| (n * multiplier as f64) as u64)
}

/// Parse a duration string like "1d", "7d", "1h", "30m" to Duration
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim().to_lowercase();

    if s.is_empty() {
        return None;
    }

    // Try to find the unit suffix
    let (num_str, multiplier) = if s.ends_with('d') {
        (&s[..s.len() - 1], 86400u64) // days
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 3600u64) // hours
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 60u64) // minutes
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], 1u64) // seconds
    } else if s.ends_with('w') {
        (&s[..s.len() - 1], 604800u64) // weeks
    } else {
        // Assume seconds if no unit
        (s.as_str(), 1u64)
    };

    num_str
        .trim()
        .parse::<f64>()
        .ok()
        .map(|n| Duration::from_secs((n * multiplier as f64) as u64))
}

/// Format a duration as a human-readable string
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();

    if secs >= 604800 && secs % 604800 == 0 {
        format!("{}w", secs / 604800)
    } else if secs >= 86400 && secs % 86400 == 0 {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 && secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 && secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

/// Format bytes as a human-readable size string
pub fn format_size(bytes: u64) -> String {
    const TB: u64 = 1024 * 1024 * 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    const KB: u64 = 1024;

    if bytes >= TB && bytes % TB == 0 {
        format!("{}TB", bytes / TB)
    } else if bytes >= GB && bytes % GB == 0 {
        format!("{}GB", bytes / GB)
    } else if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB && bytes % MB == 0 {
        format!("{}MB", bytes / MB)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB && bytes % KB == 0 {
        format!("{}KB", bytes / KB)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("50GB"), Some(50 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("1TB"), Some(1024 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("500MB"), Some(500 * 1024 * 1024));
        assert_eq!(parse_size("100KB"), Some(100 * 1024));
        assert_eq!(parse_size("1024B"), Some(1024));
        assert_eq!(
            parse_size("1.5GB"),
            Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64)
        );
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86400)));
        assert_eq!(parse_duration("7d"), Some(Duration::from_secs(7 * 86400)));
        assert_eq!(parse_duration("30d"), Some(Duration::from_secs(30 * 86400)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_duration("60s"), Some(Duration::from_secs(60)));
        assert_eq!(parse_duration("1w"), Some(Duration::from_secs(604800)));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(86400)), "1d");
        assert_eq!(format_duration(Duration::from_secs(604800)), "1w");
        assert_eq!(format_duration(Duration::from_secs(3600)), "1h");
        assert_eq!(format_duration(Duration::from_secs(60)), "1m");
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1GB");
        assert_eq!(format_size(50 * 1024 * 1024 * 1024), "50GB");
        assert_eq!(format_size(1024 * 1024 * 1024 * 1024), "1TB");
        assert_eq!(format_size(500 * 1024 * 1024), "500MB");
    }

    #[test]
    fn test_ilm_config_from_toml() {
        let toml_str = r#"
enabled = true
check_interval_secs = 30

[policies.logs]
description = "Log retention policy"
rollover_max_size = "50GB"
rollover_max_age = "1d"
rollover_max_docs = 10000000

[policies.logs.phases.hot]
min_age = "0d"

[policies.logs.phases.warm]
min_age = "7d"
readonly = true

[policies.logs.phases.cold]
min_age = "30d"
storage = "s3"

[policies.logs.phases.delete]
min_age = "90d"
"#;

        let config: IlmConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert_eq!(config.check_interval_secs, 30);
        assert!(config.policies.contains_key("logs"));

        let logs_config = config.policies.get("logs").unwrap();
        assert_eq!(logs_config.rollover_max_size, Some("50GB".to_string()));
        assert_eq!(logs_config.rollover_max_docs, Some(10_000_000));

        // Build actual policies
        let policies = config.build_policies();
        let logs_policy = policies.get("logs").unwrap();

        assert_eq!(logs_policy.rollover.max_size, Some(50 * 1024 * 1024 * 1024));
        assert_eq!(logs_policy.rollover.max_docs, Some(10_000_000));

        let warm = logs_policy.phases.get(&Phase::Warm).unwrap();
        assert!(warm.readonly);
        assert_eq!(warm.min_age, Duration::from_secs(7 * 86400));

        let cold = logs_policy.phases.get(&Phase::Cold).unwrap();
        assert_eq!(cold.storage, Some(StorageTier::S3));
        assert_eq!(cold.min_age, Duration::from_secs(30 * 86400));
    }
}
