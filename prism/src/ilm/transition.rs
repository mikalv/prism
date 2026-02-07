//! Phase transition logic for ILM
//!
//! Handles transitioning indexes between lifecycle phases.

use super::types::{IlmPolicy, IlmState, Phase, StorageTier};
use crate::collection::CollectionManager;
use crate::{Error, Result};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Result of a phase transition check
#[derive(Debug)]
pub struct TransitionCheckResult {
    /// Whether a transition should occur
    pub should_transition: bool,

    /// Current phase
    pub current_phase: Phase,

    /// Target phase (if transition should occur)
    pub target_phase: Option<Phase>,

    /// Age since rollover
    pub age_secs: u64,
}

/// Result of a phase transition
#[derive(Debug, Clone, serde::Serialize)]
pub struct TransitionResult {
    /// Collection name
    pub collection: String,

    /// Previous phase
    pub from_phase: Phase,

    /// New phase
    pub to_phase: Phase,

    /// Actions taken
    pub actions: Vec<TransitionAction>,
}

/// Actions taken during phase transition
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransitionAction {
    /// Made index read-only
    SetReadonly,
    /// Changed storage tier
    MigrateStorage { from: StorageTier, to: StorageTier },
    /// Force merged segments
    ForceMerge { target_segments: usize },
    /// Marked for deletion
    MarkForDeletion,
}

/// Service for handling phase transitions
pub struct TransitionService {
    /// Reference to collection manager
    manager: Arc<CollectionManager>,

    /// ILM state
    state: Arc<RwLock<IlmState>>,
}

impl TransitionService {
    /// Create a new transition service
    pub fn new(manager: Arc<CollectionManager>, state: Arc<RwLock<IlmState>>) -> Self {
        Self { manager, state }
    }

    /// Check if an index should transition to a new phase
    pub async fn check_transition(
        &self,
        collection_name: &str,
        policy: &IlmPolicy,
    ) -> Result<TransitionCheckResult> {
        let state = self.state.read().await;

        let managed = state
            .get(collection_name)
            .ok_or_else(|| Error::Ilm(format!("Collection '{}' not managed by ILM", collection_name)))?;

        let current_phase = managed.phase;
        let age = managed.age_since_rollover();

        let target_phase = policy.should_transition(current_phase, age);

        Ok(TransitionCheckResult {
            should_transition: target_phase.is_some(),
            current_phase,
            target_phase,
            age_secs: age.as_secs(),
        })
    }

    /// Execute a phase transition
    pub async fn execute_transition(
        &self,
        collection_name: &str,
        target_phase: Phase,
        policy: &IlmPolicy,
    ) -> Result<TransitionResult> {
        let mut state = self.state.write().await;

        let managed = state
            .get_mut(collection_name)
            .ok_or_else(|| Error::Ilm(format!("Collection '{}' not managed by ILM", collection_name)))?;

        let from_phase = managed.phase;
        let mut actions = Vec::new();

        // Get phase config
        let phase_config = policy.phase_config(target_phase);

        // Apply phase actions
        if let Some(config) = phase_config {
            // Set readonly if configured
            if config.readonly && !managed.readonly {
                managed.readonly = true;
                actions.push(TransitionAction::SetReadonly);
                tracing::info!("Set collection '{}' to read-only", collection_name);
            }

            // Handle storage tier migration
            if let Some(ref new_tier) = config.storage {
                if managed.storage_tier != *new_tier {
                    let old_tier = managed.storage_tier.clone();
                    actions.push(TransitionAction::MigrateStorage {
                        from: old_tier.clone(),
                        to: new_tier.clone(),
                    });

                    // Trigger async migration (this would be handled by migration.rs)
                    self.schedule_storage_migration(collection_name, new_tier)
                        .await?;

                    managed.storage_tier = new_tier.clone();
                    tracing::info!(
                        "Scheduled storage migration for '{}': {} -> {}",
                        collection_name,
                        old_tier,
                        new_tier
                    );
                }
            }

            // Handle force merge
            if let Some(target_segments) = config.force_merge_segments {
                actions.push(TransitionAction::ForceMerge { target_segments });
                self.schedule_force_merge(collection_name, target_segments)
                    .await?;
                tracing::info!(
                    "Scheduled force merge for '{}' to {} segments",
                    collection_name,
                    target_segments
                );
            }
        }

        // Handle delete phase
        if target_phase == Phase::Delete {
            actions.push(TransitionAction::MarkForDeletion);
            tracing::info!("Marked '{}' for deletion", collection_name);
        }

        // Update phase
        managed.transition_to(target_phase);
        managed.clear_error();

        tracing::info!(
            "Transitioned '{}' from {} to {}",
            collection_name,
            from_phase,
            target_phase
        );

        Ok(TransitionResult {
            collection: collection_name.to_string(),
            from_phase,
            to_phase: target_phase,
            actions,
        })
    }

    /// Force a transition to a specific phase
    pub async fn force_transition(
        &self,
        collection_name: &str,
        target_phase: Phase,
        policy: &IlmPolicy,
    ) -> Result<TransitionResult> {
        // Same as execute but without age checks
        self.execute_transition(collection_name, target_phase, policy)
            .await
    }

    /// Schedule storage migration (placeholder - would be handled by migration.rs)
    async fn schedule_storage_migration(
        &self,
        _collection_name: &str,
        _target_tier: &StorageTier,
    ) -> Result<()> {
        // This would queue a background migration job
        // For now, just log the intent
        Ok(())
    }

    /// Schedule force merge (placeholder)
    async fn schedule_force_merge(
        &self,
        _collection_name: &str,
        _target_segments: usize,
    ) -> Result<()> {
        // This would trigger a force merge on the text backend
        // For now, just log the intent
        Ok(())
    }

    /// Execute pending deletions for indexes in delete phase
    pub async fn execute_deletions(&self) -> Result<Vec<String>> {
        let state = self.state.read().await;
        let mut deleted = Vec::new();

        // Find all indexes in delete phase
        let to_delete: Vec<String> = state
            .indexes
            .values()
            .filter(|idx| idx.phase == Phase::Delete)
            .map(|idx| idx.collection_name.clone())
            .collect();

        drop(state);

        for collection_name in to_delete {
            match self.delete_index(&collection_name).await {
                Ok(()) => {
                    deleted.push(collection_name);
                }
                Err(e) => {
                    tracing::error!("Failed to delete '{}': {:?}", collection_name, e);
                    // Record error in state
                    let mut state = self.state.write().await;
                    if let Some(idx) = state.get_mut(&collection_name) {
                        idx.set_error(e.to_string());
                    }
                }
            }
        }

        Ok(deleted)
    }

    /// Delete an index and its data
    async fn delete_index(&self, collection_name: &str) -> Result<()> {
        // Remove from ILM state
        let mut state = self.state.write().await;
        state.remove(collection_name);
        drop(state);

        // Delete the actual index data
        // This would need to interact with the storage layer
        tracing::info!("Deleted index '{}'", collection_name);

        Ok(())
    }

    /// Get the current phase of an index
    pub async fn get_phase(&self, collection_name: &str) -> Option<Phase> {
        let state = self.state.read().await;
        state.get(collection_name).map(|idx| idx.phase)
    }

    /// Check if an index is read-only
    pub async fn is_readonly(&self, collection_name: &str) -> bool {
        let state = self.state.read().await;
        state
            .get(collection_name)
            .map(|idx| idx.readonly)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::{TextBackend, VectorBackend};
    use crate::ilm::types::{ManagedIndex, PhaseConfig, RolloverConditions};
    use std::collections::HashMap;
    use std::time::Duration;
    use tempfile::TempDir;

    async fn create_test_setup(temp: &TempDir) -> (Arc<CollectionManager>, Arc<RwLock<IlmState>>) {
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&schemas_dir).unwrap();

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
"#,
        )
        .unwrap();

        let text_backend = Arc::new(TextBackend::new(&data_dir).unwrap());
        let vector_backend = Arc::new(VectorBackend::new(&data_dir).unwrap());
        let manager =
            Arc::new(CollectionManager::new(&schemas_dir, text_backend, vector_backend).unwrap());
        manager.initialize().await.unwrap();

        let state = Arc::new(RwLock::new(IlmState::new()));

        (manager, state)
    }

    fn create_test_policy() -> IlmPolicy {
        let mut phases = HashMap::new();

        phases.insert(
            Phase::Hot,
            PhaseConfig {
                min_age: Duration::from_secs(0),
                readonly: false,
                storage: None,
                force_merge_segments: None,
                shrink_shards: None,
            },
        );

        phases.insert(
            Phase::Warm,
            PhaseConfig {
                min_age: Duration::from_secs(86400), // 1 day
                readonly: true,
                storage: None,
                force_merge_segments: Some(1),
                shrink_shards: None,
            },
        );

        phases.insert(
            Phase::Cold,
            PhaseConfig {
                min_age: Duration::from_secs(604800), // 7 days
                readonly: true,
                storage: Some(StorageTier::S3),
                force_merge_segments: None,
                shrink_shards: None,
            },
        );

        IlmPolicy {
            name: "test".to_string(),
            description: String::new(),
            rollover: RolloverConditions::default(),
            phases,
        }
    }

    #[tokio::test]
    async fn test_transition_check() {
        let temp = TempDir::new().unwrap();
        let (manager, state) = create_test_setup(&temp).await;

        // Add a managed index
        {
            let mut s = state.write().await;
            let idx = ManagedIndex::new("test-000001", "test", "test-policy", 1);
            s.upsert(idx);
        }

        let service = TransitionService::new(manager, state);
        let policy = create_test_policy();

        // Check for transition (should not transition yet - too young)
        let result = service.check_transition("test-000001", &policy).await.unwrap();
        assert!(!result.should_transition);
        assert_eq!(result.current_phase, Phase::Hot);
    }

    #[tokio::test]
    async fn test_force_transition() {
        let temp = TempDir::new().unwrap();
        let (manager, state) = create_test_setup(&temp).await;

        // Add a managed index
        {
            let mut s = state.write().await;
            let idx = ManagedIndex::new("test-000001", "test", "test-policy", 1);
            s.upsert(idx);
        }

        let service = TransitionService::new(manager, state.clone());
        let policy = create_test_policy();

        // Force transition to warm
        let result = service
            .force_transition("test-000001", Phase::Warm, &policy)
            .await
            .unwrap();

        assert_eq!(result.from_phase, Phase::Hot);
        assert_eq!(result.to_phase, Phase::Warm);
        assert!(result.actions.iter().any(|a| matches!(a, TransitionAction::SetReadonly)));

        // Verify state
        let s = state.read().await;
        let idx = s.get("test-000001").unwrap();
        assert_eq!(idx.phase, Phase::Warm);
        assert!(idx.readonly);
    }
}
