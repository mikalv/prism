//! Rollover service for ILM
//!
//! Handles checking rollover conditions and executing rollover operations.

use super::alias::AliasManager;
use super::types::{IlmPolicy, IlmState, ManagedIndex, RolloverReason};
use crate::backends::BackendStats;
use crate::collection::CollectionManager;
use crate::schema::CollectionSchema;
use crate::{Error, Result};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Result of a rollover check
#[derive(Debug)]
pub struct RolloverCheckResult {
    /// Whether rollover should happen
    pub should_rollover: bool,

    /// Reasons for rollover
    pub reasons: Vec<RolloverReason>,

    /// Current index stats
    pub stats: BackendStats,
}

/// Result of a rollover operation
#[derive(Debug, Clone, serde::Serialize)]
pub struct RolloverResult {
    /// Old index name that was rolled over
    pub old_index: String,

    /// New index name that was created
    pub new_index: String,

    /// Reasons for rollover
    pub reasons: Vec<RolloverReason>,

    /// Whether this was a manual rollover
    pub manual: bool,
}

/// Service for handling index rollover
pub struct RolloverService {
    /// Reference to collection manager
    manager: Arc<CollectionManager>,

    /// Alias manager
    alias_manager: Arc<AliasManager>,

    /// ILM state
    state: Arc<RwLock<IlmState>>,
}

impl RolloverService {
    /// Create a new rollover service
    pub fn new(
        manager: Arc<CollectionManager>,
        alias_manager: Arc<AliasManager>,
        state: Arc<RwLock<IlmState>>,
    ) -> Self {
        Self {
            manager,
            alias_manager,
            state,
        }
    }

    /// Check if an index should be rolled over
    pub async fn check_rollover(
        &self,
        collection_name: &str,
        policy: &IlmPolicy,
    ) -> Result<RolloverCheckResult> {
        // Get current stats
        let stats = self.manager.stats(collection_name).await?;

        // Get managed index state
        let state = self.state.read().await;
        let managed = state.get(collection_name);

        let age = if let Some(idx) = managed {
            idx.age()
        } else {
            std::time::Duration::from_secs(0)
        };

        // Check rollover conditions
        let reasons =
            policy
                .rollover
                .check_conditions(stats.size_bytes as u64, stats.document_count, age);

        Ok(RolloverCheckResult {
            should_rollover: !reasons.is_empty(),
            reasons,
            stats,
        })
    }

    /// Execute a rollover operation
    pub async fn execute_rollover(
        &self,
        index_name: &str,
        policy: &IlmPolicy,
        reasons: Vec<RolloverReason>,
        manual: bool,
    ) -> Result<RolloverResult> {
        let mut state = self.state.write().await;

        // Get current write target
        let old_collection = self.alias_manager.resolve_write_target(index_name).await?;

        // Get or create managed index entry
        let current_gen = state.latest_generation(index_name);
        let new_gen = current_gen + 1;

        // Generate new collection name
        let new_collection = ManagedIndex::generate_collection_name(index_name, new_gen);

        // Get schema from old collection
        let schema = self
            .manager
            .get_schema(&old_collection)
            .ok_or_else(|| Error::CollectionNotFound(old_collection.clone()))?
            .clone();

        // Create the new collection with the same schema
        self.create_collection_from_schema(&new_collection, &schema)
            .await?;

        // Update aliases atomically:
        // 1. Write alias points to new collection
        // 2. Read alias includes both old and new
        self.alias_manager
            .update_write_target(index_name, &new_collection)
            .await?;
        self.alias_manager
            .add_read_target(index_name, &new_collection)
            .await?;

        // Mark old index as rolled over
        if let Some(old_idx) = state.get_mut(&old_collection) {
            old_idx.mark_rolled_over();
            old_idx.readonly = true;
        }

        // Create managed index entry for new collection
        let new_managed = ManagedIndex::new(&new_collection, index_name, &policy.name, new_gen);
        state.upsert(new_managed);

        tracing::info!(
            "Rolled over index '{}': {} -> {} (reasons: {:?})",
            index_name,
            old_collection,
            new_collection,
            reasons
        );

        Ok(RolloverResult {
            old_index: old_collection,
            new_index: new_collection,
            reasons,
            manual,
        })
    }

    /// Create a new collection from an existing schema
    async fn create_collection_from_schema(
        &self,
        collection_name: &str,
        schema: &CollectionSchema,
    ) -> Result<()> {
        // Create a modified schema with the new collection name
        let mut new_schema = schema.clone();
        new_schema.collection = collection_name.to_string();

        // Save schema file
        let schemas_dir = self.get_schemas_dir()?;
        let schema_path = schemas_dir.join(format!("{}.yaml", collection_name));

        let yaml = serde_yaml::to_string(&new_schema)?;
        tokio::fs::write(&schema_path, yaml).await?;

        tracing::debug!("Created schema file: {:?}", schema_path);

        Ok(())
    }

    /// Get the schemas directory (we need this from config)
    fn get_schemas_dir(&self) -> Result<std::path::PathBuf> {
        // Try common locations - in practice this would come from config
        let candidates = [
            std::path::PathBuf::from("schemas"),
            std::path::PathBuf::from("./schemas"),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".prismsearch")
                .join("schemas"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return Ok(candidate.clone());
            }
        }

        // Default to first option
        Ok(candidates[0].clone())
    }

    /// Manually trigger a rollover
    pub async fn manual_rollover(
        &self,
        index_name: &str,
        policy: &IlmPolicy,
    ) -> Result<RolloverResult> {
        self.execute_rollover(index_name, policy, vec![RolloverReason::Manual], true)
            .await
    }

    /// Initialize a new managed index (first index for a policy)
    pub async fn initialize_index(
        &self,
        index_name: &str,
        policy: &IlmPolicy,
        collection_name: Option<&str>,
    ) -> Result<ManagedIndex> {
        let mut state = self.state.write().await;

        let gen = 1u32;
        let collection = collection_name
            .map(|s| s.to_string())
            .unwrap_or_else(|| ManagedIndex::generate_collection_name(index_name, gen));

        // Create managed index
        let managed = ManagedIndex::new(&collection, index_name, &policy.name, gen);

        // Set up aliases
        self.alias_manager
            .get_or_create_write_alias(index_name, &collection)
            .await?;
        self.alias_manager
            .get_or_create_read_alias(index_name, vec![collection.clone()])
            .await?;

        state.upsert(managed.clone());

        tracing::info!(
            "Initialized managed index '{}' with collection '{}'",
            index_name,
            collection
        );

        Ok(managed)
    }

    /// Get managed index state
    pub async fn get_managed_index(&self, collection_name: &str) -> Option<ManagedIndex> {
        let state = self.state.read().await;
        state.get(collection_name).cloned()
    }

    /// Get all managed indexes for an index name
    pub async fn get_indexes_for(&self, index_name: &str) -> Vec<ManagedIndex> {
        let state = self.state.read().await;
        state.indexes_for(index_name).into_iter().cloned().collect()
    }

    /// Update managed index state
    pub async fn update_managed_index(&self, index: ManagedIndex) {
        let mut state = self.state.write().await;
        state.upsert(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::{TextBackend, VectorBackend};
    use crate::ilm::types::{ManagedIndex, RolloverConditions};
    use std::time::Duration;
    use tempfile::TempDir;

    async fn create_test_manager(temp: &TempDir) -> Arc<CollectionManager> {
        let schemas_dir = temp.path().join("schemas");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&schemas_dir).unwrap();

        // Create a test schema
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
        let manager = Arc::new(
            CollectionManager::new(&schemas_dir, text_backend, vector_backend, None).unwrap(),
        );
        manager.initialize().await.unwrap();
        manager
    }

    fn make_policy() -> IlmPolicy {
        IlmPolicy {
            name: "test".to_string(),
            description: String::new(),
            rollover: RolloverConditions {
                max_docs: Some(100),
                max_size: None,
                max_age: Some(Duration::from_secs(86400)),
            },
            phases: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_rollover_check() {
        let temp = TempDir::new().unwrap();
        let manager = create_test_manager(&temp).await;
        let alias_manager = Arc::new(AliasManager::new(temp.path()).await.unwrap());
        let state = Arc::new(RwLock::new(IlmState::new()));

        let service = RolloverService::new(manager, alias_manager, state);

        let policy = make_policy();

        // Check rollover for empty collection
        let result = service.check_rollover("test", &policy).await.unwrap();
        assert!(!result.should_rollover);
        assert!(result.reasons.is_empty());
    }

    // ========================================================================
    // initialize_index tests
    // ========================================================================

    #[tokio::test]
    async fn test_initialize_index_with_explicit_collection() {
        let temp = TempDir::new().unwrap();
        let manager = create_test_manager(&temp).await;
        let alias_manager = Arc::new(AliasManager::new(temp.path()).await.unwrap());
        let state = Arc::new(RwLock::new(IlmState::new()));

        let service = RolloverService::new(manager, alias_manager.clone(), state.clone());
        let policy = make_policy();

        let managed = service
            .initialize_index("test", &policy, Some("test"))
            .await
            .unwrap();

        assert_eq!(managed.index_name, "test");
        assert_eq!(managed.collection_name, "test");
        assert_eq!(managed.policy_name, "test");
        assert_eq!(managed.generation, 1);
        assert_eq!(managed.phase, crate::ilm::types::Phase::Hot);
        assert!(!managed.readonly);

        // Verify aliases were created
        let write_alias = alias_manager.get_write_alias("test").await;
        assert!(write_alias.is_some());
        assert_eq!(write_alias.unwrap().targets, vec!["test"]);

        let read_alias = alias_manager.get_read_alias("test").await;
        assert!(read_alias.is_some());
        assert_eq!(read_alias.unwrap().targets, vec!["test"]);
    }

    #[tokio::test]
    async fn test_initialize_index_auto_generated_name() {
        let temp = TempDir::new().unwrap();
        let manager = create_test_manager(&temp).await;
        let alias_manager = Arc::new(AliasManager::new(temp.path()).await.unwrap());
        let state = Arc::new(RwLock::new(IlmState::new()));

        let service = RolloverService::new(manager, alias_manager.clone(), state.clone());
        let policy = make_policy();

        let managed = service
            .initialize_index("logs", &policy, None)
            .await
            .unwrap();

        // Collection name should be auto-generated
        assert!(managed.collection_name.starts_with("logs-"));
        assert!(managed.collection_name.ends_with("-000001"));
        assert_eq!(managed.index_name, "logs");
    }

    // ========================================================================
    // get_managed_index / get_indexes_for / update_managed_index
    // ========================================================================

    #[tokio::test]
    async fn test_get_managed_index() {
        let temp = TempDir::new().unwrap();
        let manager = create_test_manager(&temp).await;
        let alias_manager = Arc::new(AliasManager::new(temp.path()).await.unwrap());
        let state = Arc::new(RwLock::new(IlmState::new()));

        let service = RolloverService::new(manager, alias_manager, state);
        let policy = make_policy();

        // No managed index yet
        assert!(service.get_managed_index("test").await.is_none());

        // Initialize
        service
            .initialize_index("test", &policy, Some("test"))
            .await
            .unwrap();

        // Now it should exist
        let managed = service.get_managed_index("test").await;
        assert!(managed.is_some());
        let managed = managed.unwrap();
        assert_eq!(managed.collection_name, "test");
    }

    #[tokio::test]
    async fn test_get_indexes_for() {
        let temp = TempDir::new().unwrap();
        let manager = create_test_manager(&temp).await;
        let alias_manager = Arc::new(AliasManager::new(temp.path()).await.unwrap());
        let state = Arc::new(RwLock::new(IlmState::new()));

        let service = RolloverService::new(manager, alias_manager, state);
        let policy = make_policy();

        // No indexes initially
        let indexes = service.get_indexes_for("test").await;
        assert!(indexes.is_empty());

        // Initialize an index
        service
            .initialize_index("test", &policy, Some("test-collection"))
            .await
            .unwrap();

        let indexes = service.get_indexes_for("test").await;
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].index_name, "test");
    }

    #[tokio::test]
    async fn test_update_managed_index() {
        let temp = TempDir::new().unwrap();
        let manager = create_test_manager(&temp).await;
        let alias_manager = Arc::new(AliasManager::new(temp.path()).await.unwrap());
        let state = Arc::new(RwLock::new(IlmState::new()));

        let service = RolloverService::new(manager, alias_manager, state);
        let policy = make_policy();

        service
            .initialize_index("test", &policy, Some("test"))
            .await
            .unwrap();

        // Get the managed index and modify it
        let mut managed = service.get_managed_index("test").await.unwrap();
        managed.readonly = true;
        managed.set_error("test error");

        service.update_managed_index(managed).await;

        // Verify the update persisted
        let updated = service.get_managed_index("test").await.unwrap();
        assert!(updated.readonly);
        assert_eq!(updated.error, Some("test error".to_string()));
    }

    // ========================================================================
    // rollover check with state (managed index age tracking)
    // ========================================================================

    #[tokio::test]
    async fn test_rollover_check_with_managed_state() {
        let temp = TempDir::new().unwrap();
        let manager = create_test_manager(&temp).await;
        let alias_manager = Arc::new(AliasManager::new(temp.path()).await.unwrap());
        let state = Arc::new(RwLock::new(IlmState::new()));

        // Add a managed index to the state
        {
            let mut s = state.write().await;
            let idx = ManagedIndex::new("test", "test", "test-policy", 1);
            s.upsert(idx);
        }

        let service = RolloverService::new(manager, alias_manager, state);
        let policy = make_policy();

        // With zero docs and age < 1d, should not rollover
        let result = service.check_rollover("test", &policy).await.unwrap();
        assert!(!result.should_rollover);
        assert!(result.reasons.is_empty());
        assert_eq!(result.stats.document_count, 0);
    }

    #[tokio::test]
    async fn test_rollover_check_nonexistent_collection() {
        let temp = TempDir::new().unwrap();
        let manager = create_test_manager(&temp).await;
        let alias_manager = Arc::new(AliasManager::new(temp.path()).await.unwrap());
        let state = Arc::new(RwLock::new(IlmState::new()));

        let service = RolloverService::new(manager, alias_manager, state);
        let policy = make_policy();

        // Checking rollover for a collection that doesn't exist in the backend should error
        let result = service.check_rollover("nonexistent", &policy).await;
        assert!(result.is_err());
    }

    // ========================================================================
    // RolloverConditions unit tests
    // ========================================================================

    #[test]
    fn test_rollover_conditions_check_multiple_reasons() {
        let conditions = RolloverConditions {
            max_size: Some(1000),
            max_docs: Some(10),
            max_age: Some(Duration::from_secs(60)),
        };

        // All conditions exceeded
        let reasons = conditions.check_conditions(2000, 20, Duration::from_secs(120));
        assert_eq!(reasons.len(), 3);
    }

    #[test]
    fn test_rollover_conditions_no_conditions() {
        let conditions = RolloverConditions {
            max_size: None,
            max_docs: None,
            max_age: None,
        };

        assert!(!conditions.should_rollover(999999, 999999, Duration::from_secs(999999)));
        let reasons = conditions.check_conditions(999999, 999999, Duration::from_secs(999999));
        assert!(reasons.is_empty());
    }
}
