//! ILM types for index lifecycle management
//!
//! This module defines the core types for managing index lifecycles:
//! - Phases (hot, warm, cold, frozen, delete)
//! - Rollover conditions (size, age, doc count)
//! - Phase configurations
//! - Managed index state

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Index lifecycle phases
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Active writes, high-performance storage
    Hot,
    /// Read-optimized, no writes
    Warm,
    /// Infrequent access, can be on slower/cheaper storage
    Cold,
    /// Rarely accessed, minimal resources
    Frozen,
    /// Ready for deletion
    Delete,
}

impl Phase {
    /// Get the phase order (lower = earlier in lifecycle)
    pub fn order(&self) -> u8 {
        match self {
            Phase::Hot => 0,
            Phase::Warm => 1,
            Phase::Cold => 2,
            Phase::Frozen => 3,
            Phase::Delete => 4,
        }
    }

    /// Get all phases in lifecycle order
    pub fn all_phases() -> &'static [Phase] {
        &[
            Phase::Hot,
            Phase::Warm,
            Phase::Cold,
            Phase::Frozen,
            Phase::Delete,
        ]
    }

    /// Get the next phase in the lifecycle, if any
    pub fn next(&self) -> Option<Phase> {
        match self {
            Phase::Hot => Some(Phase::Warm),
            Phase::Warm => Some(Phase::Cold),
            Phase::Cold => Some(Phase::Frozen),
            Phase::Frozen => Some(Phase::Delete),
            Phase::Delete => None,
        }
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Hot => write!(f, "hot"),
            Phase::Warm => write!(f, "warm"),
            Phase::Cold => write!(f, "cold"),
            Phase::Frozen => write!(f, "frozen"),
            Phase::Delete => write!(f, "delete"),
        }
    }
}

impl std::str::FromStr for Phase {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "hot" => Ok(Phase::Hot),
            "warm" => Ok(Phase::Warm),
            "cold" => Ok(Phase::Cold),
            "frozen" => Ok(Phase::Frozen),
            "delete" => Ok(Phase::Delete),
            _ => Err(format!("Unknown phase: {}", s)),
        }
    }
}

/// Storage tier for data placement
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageTier {
    /// Local SSD/disk storage
    Local,
    /// S3 or compatible object storage
    S3,
}

impl Default for StorageTier {
    fn default() -> Self {
        StorageTier::Local
    }
}

impl std::fmt::Display for StorageTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageTier::Local => write!(f, "local"),
            StorageTier::S3 => write!(f, "s3"),
        }
    }
}

/// Conditions that trigger index rollover
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RolloverConditions {
    /// Maximum index size in bytes (e.g., 50GB = 53687091200)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,

    /// Maximum age since index creation
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(with = "optional_duration_serde")]
    pub max_age: Option<Duration>,

    /// Maximum document count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_docs: Option<usize>,
}

impl RolloverConditions {
    /// Check if any rollover condition is met
    pub fn should_rollover(&self, size_bytes: u64, doc_count: usize, age: Duration) -> bool {
        if let Some(max_size) = self.max_size {
            if size_bytes >= max_size {
                return true;
            }
        }
        if let Some(max_docs) = self.max_docs {
            if doc_count >= max_docs {
                return true;
            }
        }
        if let Some(max_age) = self.max_age {
            if age >= max_age {
                return true;
            }
        }
        false
    }

    /// Get which condition(s) would trigger rollover
    pub fn check_conditions(
        &self,
        size_bytes: u64,
        doc_count: usize,
        age: Duration,
    ) -> Vec<RolloverReason> {
        let mut reasons = Vec::new();

        if let Some(max_size) = self.max_size {
            if size_bytes >= max_size {
                reasons.push(RolloverReason::MaxSize {
                    current: size_bytes,
                    limit: max_size,
                });
            }
        }
        if let Some(max_docs) = self.max_docs {
            if doc_count >= max_docs {
                reasons.push(RolloverReason::MaxDocs {
                    current: doc_count,
                    limit: max_docs,
                });
            }
        }
        if let Some(max_age) = self.max_age {
            if age >= max_age {
                reasons.push(RolloverReason::MaxAge {
                    current: age,
                    limit: max_age,
                });
            }
        }

        reasons
    }
}

/// Reason(s) for rollover
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RolloverReason {
    MaxSize {
        current: u64,
        limit: u64,
    },
    MaxDocs {
        current: usize,
        limit: usize,
    },
    MaxAge {
        #[serde(with = "duration_serde")]
        current: Duration,
        #[serde(with = "duration_serde")]
        limit: Duration,
    },
    Manual,
}

/// Configuration for a specific phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseConfig {
    /// Minimum age of index before transitioning to this phase
    #[serde(with = "duration_serde")]
    pub min_age: Duration,

    /// Whether the index is read-only in this phase
    #[serde(default)]
    pub readonly: bool,

    /// Storage tier for this phase
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageTier>,

    /// Force merge to this many segments (optional optimization)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_merge_segments: Option<usize>,

    /// Shrink index to this many shards (future feature)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shrink_shards: Option<usize>,
}

impl Default for PhaseConfig {
    fn default() -> Self {
        Self {
            min_age: Duration::from_secs(0),
            readonly: false,
            storage: None,
            force_merge_segments: None,
            shrink_shards: None,
        }
    }
}

/// ILM policy defining lifecycle rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IlmPolicy {
    /// Policy name
    pub name: String,

    /// Description of the policy
    #[serde(default)]
    pub description: String,

    /// Rollover conditions (triggers creation of new index)
    #[serde(default)]
    pub rollover: RolloverConditions,

    /// Phase configurations
    #[serde(default)]
    pub phases: HashMap<Phase, PhaseConfig>,
}

impl IlmPolicy {
    /// Create a new empty policy
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            rollover: RolloverConditions::default(),
            phases: HashMap::new(),
        }
    }

    /// Get the phase config for a given phase
    pub fn phase_config(&self, phase: Phase) -> Option<&PhaseConfig> {
        self.phases.get(&phase)
    }

    /// Check if a phase transition should occur based on index age
    pub fn should_transition(
        &self,
        current_phase: Phase,
        age_since_rollover: Duration,
    ) -> Option<Phase> {
        // Find the next phase that should be active based on age
        for &phase in Phase::all_phases() {
            if phase.order() <= current_phase.order() {
                continue;
            }

            if let Some(config) = self.phases.get(&phase) {
                if age_since_rollover >= config.min_age {
                    return Some(phase);
                }
            }
        }
        None
    }
}

/// State of a managed index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedIndex {
    /// Full collection name (e.g., "logs-2026.01.29-000001")
    pub collection_name: String,

    /// Base index name pattern (e.g., "logs")
    pub index_name: String,

    /// Current lifecycle phase
    pub phase: Phase,

    /// When the index was created
    pub created_at: DateTime<Utc>,

    /// When the index was rolled over (transitioned from hot to warm)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rolled_over_at: Option<DateTime<Utc>>,

    /// Policy name governing this index
    pub policy_name: String,

    /// Generation number (increments with each rollover)
    pub generation: u32,

    /// Whether the index is read-only
    #[serde(default)]
    pub readonly: bool,

    /// Current storage tier
    #[serde(default)]
    pub storage_tier: StorageTier,

    /// Last time this index was checked by ILM
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<DateTime<Utc>>,

    /// Error message if last operation failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ManagedIndex {
    /// Create a new managed index
    pub fn new(
        collection_name: impl Into<String>,
        index_name: impl Into<String>,
        policy_name: impl Into<String>,
        generation: u32,
    ) -> Self {
        Self {
            collection_name: collection_name.into(),
            index_name: index_name.into(),
            phase: Phase::Hot,
            created_at: Utc::now(),
            rolled_over_at: None,
            policy_name: policy_name.into(),
            generation,
            readonly: false,
            storage_tier: StorageTier::Local,
            last_checked_at: None,
            error: None,
        }
    }

    /// Generate the collection name for a new generation
    pub fn generate_collection_name(index_name: &str, generation: u32) -> String {
        let now = Utc::now();
        format!(
            "{}-{}-{:06}",
            index_name,
            now.format("%Y.%m.%d"),
            generation
        )
    }

    /// Get the age since creation
    pub fn age(&self) -> Duration {
        let elapsed = Utc::now().signed_duration_since(self.created_at);
        Duration::from_secs(elapsed.num_seconds().max(0) as u64)
    }

    /// Get the age since rollover (or creation if not rolled over)
    pub fn age_since_rollover(&self) -> Duration {
        let reference = self.rolled_over_at.unwrap_or(self.created_at);
        let elapsed = Utc::now().signed_duration_since(reference);
        Duration::from_secs(elapsed.num_seconds().max(0) as u64)
    }

    /// Mark the index as rolled over
    pub fn mark_rolled_over(&mut self) {
        self.rolled_over_at = Some(Utc::now());
    }

    /// Transition to a new phase
    pub fn transition_to(&mut self, phase: Phase) {
        self.phase = phase;
        self.last_checked_at = Some(Utc::now());
    }

    /// Set an error message
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    /// Clear any error
    pub fn clear_error(&mut self) {
        self.error = None;
    }
}

/// ILM state persistence
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IlmState {
    /// All managed indexes
    pub indexes: HashMap<String, ManagedIndex>,

    /// Last time state was saved
    pub last_saved_at: Option<DateTime<Utc>>,
}

impl IlmState {
    /// Create a new empty state
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a managed index
    pub fn upsert(&mut self, index: ManagedIndex) {
        self.indexes.insert(index.collection_name.clone(), index);
    }

    /// Remove a managed index
    pub fn remove(&mut self, collection_name: &str) -> Option<ManagedIndex> {
        self.indexes.remove(collection_name)
    }

    /// Get a managed index by collection name
    pub fn get(&self, collection_name: &str) -> Option<&ManagedIndex> {
        self.indexes.get(collection_name)
    }

    /// Get a mutable reference to a managed index
    pub fn get_mut(&mut self, collection_name: &str) -> Option<&mut ManagedIndex> {
        self.indexes.get_mut(collection_name)
    }

    /// Get all indexes for a given index pattern
    pub fn indexes_for(&self, index_name: &str) -> Vec<&ManagedIndex> {
        self.indexes
            .values()
            .filter(|idx| idx.index_name == index_name)
            .collect()
    }

    /// Get the latest generation for an index
    pub fn latest_generation(&self, index_name: &str) -> u32 {
        self.indexes_for(index_name)
            .iter()
            .map(|idx| idx.generation)
            .max()
            .unwrap_or(0)
    }

    /// Mark state as saved
    pub fn mark_saved(&mut self) {
        self.last_saved_at = Some(Utc::now());
    }
}

// Serde helpers for Duration
mod duration_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

mod optional_duration_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match duration {
            Some(d) => serializer.serialize_some(&d.as_secs()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<u64>::deserialize(deserializer)?;
        Ok(opt.map(Duration::from_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_order() {
        assert!(Phase::Hot.order() < Phase::Warm.order());
        assert!(Phase::Warm.order() < Phase::Cold.order());
        assert!(Phase::Cold.order() < Phase::Frozen.order());
        assert!(Phase::Frozen.order() < Phase::Delete.order());
    }

    #[test]
    fn test_phase_next() {
        assert_eq!(Phase::Hot.next(), Some(Phase::Warm));
        assert_eq!(Phase::Warm.next(), Some(Phase::Cold));
        assert_eq!(Phase::Cold.next(), Some(Phase::Frozen));
        assert_eq!(Phase::Frozen.next(), Some(Phase::Delete));
        assert_eq!(Phase::Delete.next(), None);
    }

    #[test]
    fn test_rollover_conditions() {
        let conditions = RolloverConditions {
            max_size: Some(1_000_000),
            max_docs: Some(10_000),
            max_age: Some(Duration::from_secs(86400)),
        };

        // Under limits
        assert!(!conditions.should_rollover(500_000, 5_000, Duration::from_secs(3600)));

        // Size exceeded
        assert!(conditions.should_rollover(1_500_000, 5_000, Duration::from_secs(3600)));

        // Docs exceeded
        assert!(conditions.should_rollover(500_000, 15_000, Duration::from_secs(3600)));

        // Age exceeded
        assert!(conditions.should_rollover(500_000, 5_000, Duration::from_secs(100_000)));
    }

    #[test]
    fn test_managed_index_name_generation() {
        let name = ManagedIndex::generate_collection_name("logs", 1);
        // Should match pattern: logs-YYYY.MM.DD-000001
        assert!(name.starts_with("logs-"));
        assert!(name.ends_with("-000001"));
    }

    #[test]
    fn test_policy_transition() {
        let mut policy = IlmPolicy::new("test");
        policy.phases.insert(
            Phase::Hot,
            PhaseConfig {
                min_age: Duration::from_secs(0),
                ..Default::default()
            },
        );
        policy.phases.insert(
            Phase::Warm,
            PhaseConfig {
                min_age: Duration::from_secs(86400), // 1 day
                readonly: true,
                ..Default::default()
            },
        );
        policy.phases.insert(
            Phase::Cold,
            PhaseConfig {
                min_age: Duration::from_secs(604800), // 7 days
                storage: Some(StorageTier::S3),
                ..Default::default()
            },
        );

        // In hot phase, age < 1 day -> no transition
        assert_eq!(
            policy.should_transition(Phase::Hot, Duration::from_secs(3600)),
            None
        );

        // In hot phase, age >= 1 day -> transition to warm
        assert_eq!(
            policy.should_transition(Phase::Hot, Duration::from_secs(100_000)),
            Some(Phase::Warm)
        );

        // In warm phase, age >= 7 days -> transition to cold
        assert_eq!(
            policy.should_transition(Phase::Warm, Duration::from_secs(700_000)),
            Some(Phase::Cold)
        );
    }
}
