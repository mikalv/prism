//! Index Lifecycle Management (ILM) for Prism
//!
//! Provides automatic management of index lifecycles with:
//! - Phase transitions (hot → warm → cold → frozen → delete)
//! - Rollover triggers (size, age, document count)
//! - Alias-based routing for writes and reads
//! - Storage tier migration (local → S3)
//!
//! # Configuration
//!
//! ```toml
//! [ilm]
//! enabled = true
//! check_interval_secs = 60
//!
//! [ilm.policies.logs]
//! rollover_max_size = "50GB"
//! rollover_max_age = "1d"
//! rollover_max_docs = 10000000
//!
//! [ilm.policies.logs.phases]
//! hot = { min_age = "0d" }
//! warm = { min_age = "7d", readonly = true }
//! cold = { min_age = "30d", storage = "s3" }
//! delete = { min_age = "90d" }
//! ```

pub mod alias;
pub mod config;
pub mod rollover;
pub mod transition;
pub mod types;

pub use alias::{AliasManager, AliasType, IndexAlias};
pub use config::{IlmConfig, IlmPolicyConfig};
pub use rollover::{RolloverResult, RolloverService};
pub use transition::{TransitionAction, TransitionResult, TransitionService};
pub use types::{
    IlmPolicy, IlmState, ManagedIndex, Phase, PhaseConfig, RolloverConditions, StorageTier,
};

use crate::collection::CollectionManager;
use crate::{Error, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, RwLock};
use tokio::time::interval;

/// ILM Manager - coordinates all ILM operations
pub struct IlmManager {
    /// Collection manager reference
    manager: Arc<CollectionManager>,

    /// ILM policies
    policies: Arc<RwLock<HashMap<String, IlmPolicy>>>,

    /// ILM state (managed indexes)
    state: Arc<RwLock<IlmState>>,

    /// Alias manager
    alias_manager: Arc<AliasManager>,

    /// Rollover service
    rollover_service: RolloverService,

    /// Transition service
    transition_service: TransitionService,

    /// Check interval
    check_interval: Duration,

    /// State file path
    state_path: std::path::PathBuf,

    /// Shutdown signal sender
    shutdown_tx: watch::Sender<bool>,

    /// Shutdown signal receiver
    shutdown_rx: watch::Receiver<bool>,
}

impl IlmManager {
    /// Create a new ILM manager
    pub async fn new(
        manager: Arc<CollectionManager>,
        config: &IlmConfig,
        data_dir: &Path,
    ) -> Result<Self> {
        // Create ILM directory
        let ilm_dir = data_dir.join("ilm");
        tokio::fs::create_dir_all(&ilm_dir).await?;

        // Load state
        let state_path = ilm_dir.join("state.json");
        let state = if state_path.exists() {
            let content = tokio::fs::read_to_string(&state_path).await?;
            serde_json::from_str(&content)?
        } else {
            IlmState::new()
        };
        let state = Arc::new(RwLock::new(state));

        // Build policies
        let policies = Arc::new(RwLock::new(config.build_policies()));

        // Create alias manager
        let alias_manager = Arc::new(AliasManager::new(data_dir).await?);

        // Create services
        let rollover_service =
            RolloverService::new(manager.clone(), alias_manager.clone(), state.clone());
        let transition_service = TransitionService::new(manager.clone(), state.clone());

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            manager,
            policies,
            state,
            alias_manager,
            rollover_service,
            transition_service,
            check_interval: Duration::from_secs(config.check_interval_secs),
            state_path,
            shutdown_tx,
            shutdown_rx,
        })
    }

    /// Start the background ILM processing loop
    pub async fn start(&self) -> Result<()> {
        tracing::info!(
            "ILM manager started (check interval: {:?})",
            self.check_interval
        );

        let mut interval = interval(self.check_interval);
        let mut shutdown_rx = self.shutdown_rx.clone();

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.run_cycle().await {
                        tracing::error!("ILM cycle error: {:?}", e);
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("ILM manager shutting down");
                        break;
                    }
                }
            }
        }

        // Save state on shutdown
        self.save_state().await?;

        Ok(())
    }

    /// Signal the manager to stop
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Run a single ILM cycle
    pub async fn run_cycle(&self) -> Result<()> {
        let state = self.state.read().await;
        let policies = self.policies.read().await;

        let managed_indexes: Vec<_> = state.indexes.values().cloned().collect();
        drop(state);

        for index in managed_indexes {
            // Skip if in delete phase (handled separately)
            if index.phase == Phase::Delete {
                continue;
            }

            // Get policy
            let policy = match policies.get(&index.policy_name) {
                Some(p) => p,
                None => {
                    tracing::warn!(
                        "Policy '{}' not found for index '{}'",
                        index.policy_name,
                        index.collection_name
                    );
                    continue;
                }
            };

            // Check rollover (only for hot phase)
            if index.phase == Phase::Hot {
                match self
                    .rollover_service
                    .check_rollover(&index.collection_name, policy)
                    .await
                {
                    Ok(check) if check.should_rollover => {
                        if let Err(e) = self
                            .rollover_service
                            .execute_rollover(&index.index_name, policy, check.reasons, false)
                            .await
                        {
                            tracing::error!(
                                "Rollover failed for '{}': {:?}",
                                index.collection_name,
                                e
                            );
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Rollover check failed for '{}': {:?}",
                            index.collection_name,
                            e
                        );
                    }
                    _ => {}
                }
            }

            // Check phase transition
            match self
                .transition_service
                .check_transition(&index.collection_name, policy)
                .await
            {
                Ok(check) if check.should_transition => {
                    if let Some(target) = check.target_phase {
                        if let Err(e) = self
                            .transition_service
                            .execute_transition(&index.collection_name, target, policy)
                            .await
                        {
                            tracing::error!(
                                "Transition failed for '{}': {:?}",
                                index.collection_name,
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        "Transition check failed for '{}': {:?}",
                        index.collection_name,
                        e
                    );
                }
                _ => {}
            }
        }

        // Execute pending deletions
        let deleted = self.transition_service.execute_deletions().await?;
        if !deleted.is_empty() {
            tracing::info!("Deleted {} indexes: {:?}", deleted.len(), deleted);
        }

        // Save state periodically
        self.save_state().await?;

        Ok(())
    }

    /// Save ILM state to disk
    async fn save_state(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.mark_saved();

        let content = serde_json::to_string_pretty(&*state)?;
        drop(state);

        tokio::fs::write(&self.state_path, content).await?;
        Ok(())
    }

    // ========================================================================
    // Public API
    // ========================================================================

    /// Get all ILM policies
    pub async fn list_policies(&self) -> Vec<IlmPolicy> {
        let policies = self.policies.read().await;
        policies.values().cloned().collect()
    }

    /// Get a specific policy
    pub async fn get_policy(&self, name: &str) -> Option<IlmPolicy> {
        let policies = self.policies.read().await;
        policies.get(name).cloned()
    }

    /// Create or update a policy
    pub async fn upsert_policy(&self, policy: IlmPolicy) -> Result<()> {
        let mut policies = self.policies.write().await;
        policies.insert(policy.name.clone(), policy);
        Ok(())
    }

    /// Delete a policy
    pub async fn delete_policy(&self, name: &str) -> Result<Option<IlmPolicy>> {
        let mut policies = self.policies.write().await;
        Ok(policies.remove(name))
    }

    /// Get ILM status for all managed indexes
    pub async fn get_status(&self) -> Vec<IlmIndexStatus> {
        let state = self.state.read().await;

        state
            .indexes
            .values()
            .map(|idx| IlmIndexStatus {
                collection: idx.collection_name.clone(),
                index_name: idx.index_name.clone(),
                phase: idx.phase,
                age_secs: idx.age().as_secs(),
                age_since_rollover_secs: idx.age_since_rollover().as_secs(),
                policy: idx.policy_name.clone(),
                generation: idx.generation,
                readonly: idx.readonly,
                storage_tier: idx.storage_tier.clone(),
                error: idx.error.clone(),
            })
            .collect()
    }

    /// Get ILM explain for a specific collection
    pub async fn explain(&self, collection_name: &str) -> Result<IlmExplain> {
        let state = self.state.read().await;
        let policies = self.policies.read().await;

        let managed = state.get(collection_name).ok_or_else(|| {
            Error::Ilm(format!(
                "Collection '{}' not managed by ILM",
                collection_name
            ))
        })?;

        let policy = policies
            .get(&managed.policy_name)
            .ok_or_else(|| Error::PolicyNotFound(managed.policy_name.clone()))?;

        // Check what would happen next
        let next_phase = policy.should_transition(managed.phase, managed.age_since_rollover());
        let next_phase_in = if let Some(ref target) = next_phase {
            policy.phase_config(*target).map(|c| {
                c.min_age
                    .as_secs()
                    .saturating_sub(managed.age_since_rollover().as_secs())
            })
        } else {
            None
        };

        Ok(IlmExplain {
            collection: collection_name.to_string(),
            managed: true,
            policy: managed.policy_name.clone(),
            phase: managed.phase,
            age_secs: managed.age().as_secs(),
            age_since_rollover_secs: managed.age_since_rollover().as_secs(),
            next_phase,
            next_phase_in_secs: next_phase_in,
            readonly: managed.readonly,
            storage_tier: managed.storage_tier.clone(),
        })
    }

    /// Manually trigger a rollover
    pub async fn rollover(&self, index_name: &str) -> Result<RolloverResult> {
        let policies = self.policies.read().await;

        // Find the policy for this index
        let state = self.state.read().await;
        let indexes = state.indexes_for(index_name);
        let policy_name = indexes
            .first()
            .map(|idx| idx.policy_name.clone())
            .ok_or_else(|| Error::Ilm(format!("No managed indexes for '{}'", index_name)))?;
        drop(state);

        let policy = policies
            .get(&policy_name)
            .ok_or_else(|| Error::PolicyNotFound(policy_name))?;

        self.rollover_service
            .manual_rollover(index_name, policy)
            .await
    }

    /// Force a phase transition
    pub async fn move_to_phase(
        &self,
        collection_name: &str,
        target_phase: Phase,
    ) -> Result<TransitionResult> {
        let state = self.state.read().await;
        let managed = state
            .get(collection_name)
            .ok_or_else(|| {
                Error::Ilm(format!(
                    "Collection '{}' not managed by ILM",
                    collection_name
                ))
            })?
            .clone();
        drop(state);

        let policies = self.policies.read().await;
        let policy = policies
            .get(&managed.policy_name)
            .ok_or_else(|| Error::PolicyNotFound(managed.policy_name.clone()))?
            .clone();
        drop(policies);

        self.transition_service
            .force_transition(collection_name, target_phase, &policy)
            .await
    }

    /// Initialize an index for ILM management
    pub async fn attach_policy(
        &self,
        index_name: &str,
        collection_name: &str,
        policy_name: &str,
    ) -> Result<ManagedIndex> {
        let policies = self.policies.read().await;
        let policy = policies
            .get(policy_name)
            .ok_or_else(|| Error::PolicyNotFound(policy_name.to_string()))?
            .clone();
        drop(policies);

        self.rollover_service
            .initialize_index(index_name, &policy, Some(collection_name))
            .await
    }

    /// Get alias manager
    pub fn alias_manager(&self) -> &Arc<AliasManager> {
        &self.alias_manager
    }

    /// Check if writes should be blocked for a collection
    pub async fn is_readonly(&self, collection_name: &str) -> bool {
        self.transition_service.is_readonly(collection_name).await
    }

    /// Resolve a collection name or alias to actual collection(s)
    pub async fn resolve(&self, name_or_alias: &str) -> Vec<String> {
        self.alias_manager.expand(name_or_alias).await
    }
}

/// ILM status for an index
#[derive(Debug, Clone, serde::Serialize)]
pub struct IlmIndexStatus {
    pub collection: String,
    pub index_name: String,
    pub phase: Phase,
    pub age_secs: u64,
    pub age_since_rollover_secs: u64,
    pub policy: String,
    pub generation: u32,
    pub readonly: bool,
    pub storage_tier: StorageTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// ILM explain response
#[derive(Debug, Clone, serde::Serialize)]
pub struct IlmExplain {
    pub collection: String,
    pub managed: bool,
    pub policy: String,
    pub phase: Phase,
    pub age_secs: u64,
    pub age_since_rollover_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_phase: Option<Phase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_phase_in_secs: Option<u64>,
    pub readonly: bool,
    pub storage_tier: StorageTier,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::{TextBackend, VectorBackend};
    use tempfile::TempDir;

    async fn create_test_setup(temp: &TempDir) -> (Arc<CollectionManager>, IlmConfig) {
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&schemas_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();

        std::fs::write(
            schemas_dir.join("test.yaml"),
            r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
        stored: true
"#,
        )
        .unwrap();

        let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
        let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
        let manager =
            Arc::new(CollectionManager::new(&schemas_dir, text_backend, vector_backend).unwrap());
        manager.initialize().await.unwrap();

        let config_str = r#"
enabled = true
check_interval_secs = 60

[policies.logs]
rollover_max_size = "50GB"
rollover_max_age = "1d"

[policies.logs.phases.hot]
min_age = "0d"

[policies.logs.phases.warm]
min_age = "1d"
readonly = true

[policies.logs.phases.delete]
min_age = "7d"
"#;
        let config: IlmConfig = toml::from_str(config_str).unwrap();

        (manager, config)
    }

    #[tokio::test]
    async fn test_ilm_manager_creation() {
        let temp = TempDir::new().unwrap();
        let (manager, config) = create_test_setup(&temp).await;

        let ilm = IlmManager::new(manager, &config, temp.path())
            .await
            .unwrap();

        // Check policies loaded
        let policies = ilm.list_policies().await;
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].name, "logs");
    }

    #[tokio::test]
    async fn test_ilm_attach_policy() {
        let temp = TempDir::new().unwrap();
        let (manager, config) = create_test_setup(&temp).await;

        let ilm = IlmManager::new(manager, &config, temp.path())
            .await
            .unwrap();

        // Attach policy to a collection
        let managed = ilm.attach_policy("test", "test", "logs").await.unwrap();

        assert_eq!(managed.index_name, "test");
        assert_eq!(managed.policy_name, "logs");
        assert_eq!(managed.phase, Phase::Hot);

        // Check status
        let status = ilm.get_status().await;
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].collection, "test");
    }
}
