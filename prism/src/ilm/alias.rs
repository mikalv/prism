//! Alias management for ILM
//!
//! Manages write and read aliases for index lifecycle management.
//! - Write alias: Points to the current hot index (single target)
//! - Read alias: Points to all searchable indexes (multiple targets)

use crate::{Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;
use tokio::sync::RwLock;

/// Type of alias
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AliasType {
    /// Write alias - single target for indexing
    Write,
    /// Read alias - multiple targets for searching
    Read,
}

impl std::fmt::Display for AliasType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AliasType::Write => write!(f, "write"),
            AliasType::Read => write!(f, "read"),
        }
    }
}

/// An index alias definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexAlias {
    /// Alias name (e.g., "logs-write", "logs-read")
    pub name: String,

    /// Type of alias
    pub alias_type: AliasType,

    /// Target collection names
    pub targets: Vec<String>,

    /// When the alias was created
    pub created_at: DateTime<Utc>,

    /// When the alias was last updated
    pub updated_at: DateTime<Utc>,

    /// Optional filter for read aliases (future feature)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

impl IndexAlias {
    /// Create a new write alias
    pub fn write(name: impl Into<String>, target: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            alias_type: AliasType::Write,
            targets: vec![target.into()],
            created_at: now,
            updated_at: now,
            filter: None,
        }
    }

    /// Create a new read alias
    pub fn read(name: impl Into<String>, targets: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            alias_type: AliasType::Read,
            targets,
            created_at: now,
            updated_at: now,
            filter: None,
        }
    }

    /// Get the single target for a write alias
    pub fn write_target(&self) -> Option<&str> {
        if self.alias_type == AliasType::Write {
            self.targets.first().map(|s| s.as_str())
        } else {
            None
        }
    }

    /// Add a target to the alias (for read aliases)
    pub fn add_target(&mut self, target: impl Into<String>) {
        let target = target.into();
        if !self.targets.contains(&target) {
            self.targets.push(target);
            self.updated_at = Utc::now();
        }
    }

    /// Remove a target from the alias
    pub fn remove_target(&mut self, target: &str) -> bool {
        let len_before = self.targets.len();
        self.targets.retain(|t| t != target);
        if self.targets.len() != len_before {
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    /// Set the single target (for write aliases)
    pub fn set_target(&mut self, target: impl Into<String>) {
        self.targets = vec![target.into()];
        self.updated_at = Utc::now();
    }
}

/// Alias state persistence
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AliasState {
    /// All aliases by name
    pub aliases: HashMap<String, IndexAlias>,

    /// Last saved timestamp
    pub last_saved_at: Option<DateTime<Utc>>,
}

impl AliasState {
    /// Create new empty state
    pub fn new() -> Self {
        Self::default()
    }
}

/// Manages aliases for ILM
pub struct AliasManager {
    /// Alias state
    state: RwLock<AliasState>,

    /// Path to persist alias state
    state_path: std::path::PathBuf,
}

impl AliasManager {
    /// Create a new alias manager
    pub async fn new(data_dir: &Path) -> Result<Self> {
        let ilm_dir = data_dir.join("ilm");
        fs::create_dir_all(&ilm_dir).await?;

        let state_path = ilm_dir.join("aliases.json");

        let state = if state_path.exists() {
            let content = fs::read_to_string(&state_path).await?;
            serde_json::from_str(&content)?
        } else {
            AliasState::new()
        };

        Ok(Self {
            state: RwLock::new(state),
            state_path,
        })
    }

    /// Get or create the write alias for an index
    pub async fn get_or_create_write_alias(
        &self,
        index_name: &str,
        initial_target: &str,
    ) -> Result<IndexAlias> {
        let alias_name = format!("{}-write", index_name);

        let mut state = self.state.write().await;

        if let Some(alias) = state.aliases.get(&alias_name) {
            return Ok(alias.clone());
        }

        let alias = IndexAlias::write(&alias_name, initial_target);
        state.aliases.insert(alias_name, alias.clone());

        drop(state);
        self.save().await?;

        Ok(alias)
    }

    /// Get or create the read alias for an index
    pub async fn get_or_create_read_alias(
        &self,
        index_name: &str,
        initial_targets: Vec<String>,
    ) -> Result<IndexAlias> {
        let alias_name = format!("{}-read", index_name);

        let mut state = self.state.write().await;

        if let Some(alias) = state.aliases.get(&alias_name) {
            return Ok(alias.clone());
        }

        let alias = IndexAlias::read(&alias_name, initial_targets);
        state.aliases.insert(alias_name, alias.clone());

        drop(state);
        self.save().await?;

        Ok(alias)
    }

    /// Get an alias by name
    pub async fn get(&self, alias_name: &str) -> Option<IndexAlias> {
        let state = self.state.read().await;
        state.aliases.get(alias_name).cloned()
    }

    /// Get the write alias for an index
    pub async fn get_write_alias(&self, index_name: &str) -> Option<IndexAlias> {
        self.get(&format!("{}-write", index_name)).await
    }

    /// Get the read alias for an index
    pub async fn get_read_alias(&self, index_name: &str) -> Option<IndexAlias> {
        self.get(&format!("{}-read", index_name)).await
    }

    /// Resolve an alias to its target collection(s)
    pub async fn resolve(&self, alias_name: &str) -> Result<Vec<String>> {
        let state = self.state.read().await;

        if let Some(alias) = state.aliases.get(alias_name) {
            Ok(alias.targets.clone())
        } else {
            Err(Error::AliasNotFound(alias_name.to_string()))
        }
    }

    /// Resolve the write target for an index
    pub async fn resolve_write_target(&self, index_name: &str) -> Result<String> {
        let alias_name = format!("{}-write", index_name);
        let state = self.state.read().await;

        if let Some(alias) = state.aliases.get(&alias_name) {
            alias
                .write_target()
                .map(|s| s.to_string())
                .ok_or_else(|| Error::AliasNotFound(alias_name))
        } else {
            Err(Error::AliasNotFound(alias_name))
        }
    }

    /// Resolve the read targets for an index (for multi-search)
    pub async fn resolve_read_targets(&self, index_name: &str) -> Result<Vec<String>> {
        let alias_name = format!("{}-read", index_name);
        self.resolve(&alias_name).await
    }

    /// Update the write alias to point to a new target (during rollover)
    pub async fn update_write_target(&self, index_name: &str, new_target: &str) -> Result<()> {
        let alias_name = format!("{}-write", index_name);

        let mut state = self.state.write().await;

        let alias = state
            .aliases
            .get_mut(&alias_name)
            .ok_or_else(|| Error::AliasNotFound(alias_name.clone()))?;

        alias.set_target(new_target);

        drop(state);
        self.save().await
    }

    /// Add a target to the read alias
    pub async fn add_read_target(&self, index_name: &str, target: &str) -> Result<()> {
        let alias_name = format!("{}-read", index_name);

        let mut state = self.state.write().await;

        if let Some(alias) = state.aliases.get_mut(&alias_name) {
            alias.add_target(target);
        } else {
            // Create new read alias
            let alias = IndexAlias::read(&alias_name, vec![target.to_string()]);
            state.aliases.insert(alias_name, alias);
        }

        drop(state);
        self.save().await
    }

    /// Remove a target from the read alias
    pub async fn remove_read_target(&self, index_name: &str, target: &str) -> Result<()> {
        let alias_name = format!("{}-read", index_name);

        let mut state = self.state.write().await;

        if let Some(alias) = state.aliases.get_mut(&alias_name) {
            alias.remove_target(target);
        }

        drop(state);
        self.save().await
    }

    /// List all aliases
    pub async fn list(&self) -> Vec<IndexAlias> {
        let state = self.state.read().await;
        state.aliases.values().cloned().collect()
    }

    /// List aliases for a specific index
    pub async fn list_for_index(&self, index_name: &str) -> Vec<IndexAlias> {
        let state = self.state.read().await;
        let prefix = format!("{}-", index_name);

        state
            .aliases
            .values()
            .filter(|a| a.name.starts_with(&prefix))
            .cloned()
            .collect()
    }

    /// Delete an alias
    pub async fn delete(&self, alias_name: &str) -> Result<Option<IndexAlias>> {
        let mut state = self.state.write().await;
        let removed = state.aliases.remove(alias_name);

        drop(state);
        self.save().await?;

        Ok(removed)
    }

    /// Create or update an alias
    pub async fn upsert(&self, alias: IndexAlias) -> Result<()> {
        let mut state = self.state.write().await;
        state.aliases.insert(alias.name.clone(), alias);

        drop(state);
        self.save().await
    }

    /// Perform atomic alias updates (for rollover)
    pub async fn atomic_update(
        &self,
        add: Vec<(String, String)>,    // (alias_name, target)
        remove: Vec<(String, String)>, // (alias_name, target)
    ) -> Result<()> {
        let mut state = self.state.write().await;

        // Remove targets first
        for (alias_name, target) in remove {
            if let Some(alias) = state.aliases.get_mut(&alias_name) {
                alias.remove_target(&target);
            }
        }

        // Then add targets
        for (alias_name, target) in add {
            if let Some(alias) = state.aliases.get_mut(&alias_name) {
                if alias.alias_type == AliasType::Write {
                    alias.set_target(&target);
                } else {
                    alias.add_target(&target);
                }
            }
        }

        drop(state);
        self.save().await
    }

    /// Save state to disk
    async fn save(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.last_saved_at = Some(Utc::now());

        let content = serde_json::to_string_pretty(&*state)?;
        drop(state);

        fs::write(&self.state_path, content).await?;
        Ok(())
    }

    /// Check if an alias name represents a valid alias
    pub async fn is_alias(&self, name: &str) -> bool {
        let state = self.state.read().await;
        state.aliases.contains_key(name)
    }

    /// Expand an alias or pattern to collection names
    /// Returns the input if it's not an alias
    pub async fn expand(&self, name_or_alias: &str) -> Vec<String> {
        let state = self.state.read().await;

        if let Some(alias) = state.aliases.get(name_or_alias) {
            alias.targets.clone()
        } else {
            vec![name_or_alias.to_string()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_alias_manager() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        // Create write alias
        let write_alias = manager
            .get_or_create_write_alias("logs", "logs-2026.01.29-000001")
            .await
            .unwrap();

        assert_eq!(write_alias.name, "logs-write");
        assert_eq!(write_alias.alias_type, AliasType::Write);
        assert_eq!(write_alias.targets, vec!["logs-2026.01.29-000001"]);

        // Resolve write target
        let target = manager.resolve_write_target("logs").await.unwrap();
        assert_eq!(target, "logs-2026.01.29-000001");

        // Create read alias
        let read_alias = manager
            .get_or_create_read_alias("logs", vec!["logs-2026.01.29-000001".to_string()])
            .await
            .unwrap();

        assert_eq!(read_alias.name, "logs-read");
        assert_eq!(read_alias.alias_type, AliasType::Read);

        // Add more read targets
        manager
            .add_read_target("logs", "logs-2026.01.28-000001")
            .await
            .unwrap();

        let targets = manager.resolve_read_targets("logs").await.unwrap();
        assert_eq!(targets.len(), 2);

        // Update write target (rollover)
        manager
            .update_write_target("logs", "logs-2026.01.30-000002")
            .await
            .unwrap();

        let new_target = manager.resolve_write_target("logs").await.unwrap();
        assert_eq!(new_target, "logs-2026.01.30-000002");
    }

    #[tokio::test]
    async fn test_alias_persistence() {
        let temp = TempDir::new().unwrap();

        // Create and save
        {
            let manager = AliasManager::new(temp.path()).await.unwrap();
            manager
                .get_or_create_write_alias("test", "test-000001")
                .await
                .unwrap();
        }

        // Load and verify
        {
            let manager = AliasManager::new(temp.path()).await.unwrap();
            let target = manager.resolve_write_target("test").await.unwrap();
            assert_eq!(target, "test-000001");
        }
    }

    // ========================================================================
    // resolve tests
    // ========================================================================

    #[tokio::test]
    async fn test_resolve_existing_alias() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        manager
            .get_or_create_read_alias(
                "logs",
                vec!["logs-001".to_string(), "logs-002".to_string()],
            )
            .await
            .unwrap();

        let targets = manager.resolve("logs-read").await.unwrap();
        assert_eq!(targets, vec!["logs-001", "logs-002"]);
    }

    #[tokio::test]
    async fn test_resolve_nonexistent_alias() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        let result = manager.resolve("nonexistent").await;
        assert!(result.is_err());
        // Should be AliasNotFound
        match result.unwrap_err() {
            crate::Error::AliasNotFound(name) => assert_eq!(name, "nonexistent"),
            other => panic!("Expected AliasNotFound, got: {:?}", other),
        }
    }

    // ========================================================================
    // list_aliases / list_for_index
    // ========================================================================

    #[tokio::test]
    async fn test_list_empty() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        let aliases = manager.list().await;
        assert!(aliases.is_empty());
    }

    #[tokio::test]
    async fn test_list_with_aliases() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        manager
            .get_or_create_write_alias("logs", "logs-001")
            .await
            .unwrap();
        manager
            .get_or_create_read_alias("logs", vec!["logs-001".to_string()])
            .await
            .unwrap();
        manager
            .get_or_create_write_alias("metrics", "metrics-001")
            .await
            .unwrap();

        let aliases = manager.list().await;
        assert_eq!(aliases.len(), 3);
    }

    #[tokio::test]
    async fn test_list_for_index() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        manager
            .get_or_create_write_alias("logs", "logs-001")
            .await
            .unwrap();
        manager
            .get_or_create_read_alias("logs", vec!["logs-001".to_string()])
            .await
            .unwrap();
        manager
            .get_or_create_write_alias("metrics", "metrics-001")
            .await
            .unwrap();

        let log_aliases = manager.list_for_index("logs").await;
        assert_eq!(log_aliases.len(), 2);

        let metric_aliases = manager.list_for_index("metrics").await;
        assert_eq!(metric_aliases.len(), 1);

        let empty = manager.list_for_index("nonexistent").await;
        assert!(empty.is_empty());
    }

    // ========================================================================
    // add_alias (upsert) / remove_alias (delete)
    // ========================================================================

    #[tokio::test]
    async fn test_upsert_alias() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        let alias = IndexAlias::write("my-alias", "target-001");
        manager.upsert(alias).await.unwrap();

        let retrieved = manager.get("my-alias").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().targets, vec!["target-001"]);
    }

    #[tokio::test]
    async fn test_delete_alias() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        manager
            .get_or_create_write_alias("logs", "logs-001")
            .await
            .unwrap();

        // Delete it
        let removed = manager.delete("logs-write").await.unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "logs-write");

        // Verify it's gone
        let check = manager.get("logs-write").await;
        assert!(check.is_none());

        // Deleting again returns None
        let removed_again = manager.delete("logs-write").await.unwrap();
        assert!(removed_again.is_none());
    }

    // ========================================================================
    // atomic_update
    // ========================================================================

    #[tokio::test]
    async fn test_atomic_update() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        // Set up initial state
        manager
            .get_or_create_write_alias("logs", "logs-001")
            .await
            .unwrap();
        manager
            .get_or_create_read_alias("logs", vec!["logs-001".to_string()])
            .await
            .unwrap();

        // Atomic rollover: write alias -> logs-002, read alias adds logs-002
        manager
            .atomic_update(
                vec![
                    ("logs-write".to_string(), "logs-002".to_string()),
                    ("logs-read".to_string(), "logs-002".to_string()),
                ],
                vec![],
            )
            .await
            .unwrap();

        // Verify write alias now points to logs-002
        let write_target = manager.resolve_write_target("logs").await.unwrap();
        assert_eq!(write_target, "logs-002");

        // Read alias should have both targets
        let read_targets = manager.resolve_read_targets("logs").await.unwrap();
        assert_eq!(read_targets.len(), 2);
        assert!(read_targets.contains(&"logs-001".to_string()));
        assert!(read_targets.contains(&"logs-002".to_string()));
    }

    #[tokio::test]
    async fn test_atomic_update_with_removes() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        // Set up read alias with multiple targets
        manager
            .get_or_create_read_alias(
                "logs",
                vec!["logs-001".to_string(), "logs-002".to_string()],
            )
            .await
            .unwrap();

        // Remove logs-001 from read alias
        manager
            .atomic_update(
                vec![],
                vec![("logs-read".to_string(), "logs-001".to_string())],
            )
            .await
            .unwrap();

        let targets = manager.resolve_read_targets("logs").await.unwrap();
        assert_eq!(targets, vec!["logs-002"]);
    }

    // ========================================================================
    // is_alias / expand
    // ========================================================================

    #[tokio::test]
    async fn test_is_alias() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        assert!(!manager.is_alias("logs-write").await);

        manager
            .get_or_create_write_alias("logs", "logs-001")
            .await
            .unwrap();

        assert!(manager.is_alias("logs-write").await);
        assert!(!manager.is_alias("nonexistent").await);
    }

    #[tokio::test]
    async fn test_expand_alias() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        // Expand a non-alias returns itself
        let result = manager.expand("some-collection").await;
        assert_eq!(result, vec!["some-collection".to_string()]);

        // Expand an alias returns its targets
        manager
            .get_or_create_read_alias(
                "logs",
                vec!["logs-001".to_string(), "logs-002".to_string()],
            )
            .await
            .unwrap();

        let result = manager.expand("logs-read").await;
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"logs-001".to_string()));
        assert!(result.contains(&"logs-002".to_string()));
    }

    // ========================================================================
    // remove_read_target
    // ========================================================================

    #[tokio::test]
    async fn test_remove_read_target() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        manager
            .get_or_create_read_alias(
                "logs",
                vec![
                    "logs-001".to_string(),
                    "logs-002".to_string(),
                    "logs-003".to_string(),
                ],
            )
            .await
            .unwrap();

        manager.remove_read_target("logs", "logs-002").await.unwrap();

        let targets = manager.resolve_read_targets("logs").await.unwrap();
        assert_eq!(targets.len(), 2);
        assert!(!targets.contains(&"logs-002".to_string()));
    }

    #[tokio::test]
    async fn test_remove_read_target_nonexistent_target() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        manager
            .get_or_create_read_alias("logs", vec!["logs-001".to_string()])
            .await
            .unwrap();

        // Removing a target that doesn't exist should succeed (no-op)
        manager
            .remove_read_target("logs", "logs-999")
            .await
            .unwrap();

        let targets = manager.resolve_read_targets("logs").await.unwrap();
        assert_eq!(targets, vec!["logs-001"]);
    }

    // ========================================================================
    // add_read_target creates alias if not exists
    // ========================================================================

    #[tokio::test]
    async fn test_add_read_target_creates_alias() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        // No alias exists yet, add_read_target should create one
        manager.add_read_target("logs", "logs-001").await.unwrap();

        let targets = manager.resolve_read_targets("logs").await.unwrap();
        assert_eq!(targets, vec!["logs-001"]);
    }

    #[tokio::test]
    async fn test_add_read_target_no_duplicates() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        manager
            .get_or_create_read_alias("logs", vec!["logs-001".to_string()])
            .await
            .unwrap();

        // Adding the same target again should not duplicate
        manager.add_read_target("logs", "logs-001").await.unwrap();

        let targets = manager.resolve_read_targets("logs").await.unwrap();
        assert_eq!(targets.len(), 1);
    }

    // ========================================================================
    // get_or_create returns existing
    // ========================================================================

    #[tokio::test]
    async fn test_get_or_create_returns_existing() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        let first = manager
            .get_or_create_write_alias("logs", "logs-001")
            .await
            .unwrap();
        let second = manager
            .get_or_create_write_alias("logs", "logs-002")
            .await
            .unwrap();

        // Second call should return the existing alias (not create a new one with logs-002)
        assert_eq!(first.name, second.name);
        assert_eq!(second.targets, vec!["logs-001"]);
    }

    // ========================================================================
    // IndexAlias unit tests
    // ========================================================================

    #[test]
    fn test_index_alias_write_target() {
        let alias = IndexAlias::write("test-write", "target-001");
        assert_eq!(alias.write_target(), Some("target-001"));

        let read_alias = IndexAlias::read("test-read", vec!["t1".into(), "t2".into()]);
        assert_eq!(read_alias.write_target(), None);
    }

    #[test]
    fn test_index_alias_add_target() {
        let mut alias = IndexAlias::read("test-read", vec!["t1".into()]);
        alias.add_target("t2");
        assert_eq!(alias.targets, vec!["t1", "t2"]);

        // Adding duplicate should not change anything
        alias.add_target("t2");
        assert_eq!(alias.targets, vec!["t1", "t2"]);
    }

    #[test]
    fn test_index_alias_remove_target() {
        let mut alias = IndexAlias::read("test", vec!["t1".into(), "t2".into(), "t3".into()]);

        assert!(alias.remove_target("t2"));
        assert_eq!(alias.targets, vec!["t1", "t3"]);

        // Removing nonexistent returns false
        assert!(!alias.remove_target("t99"));
    }

    #[test]
    fn test_index_alias_set_target() {
        let mut alias = IndexAlias::write("test-write", "old-target");
        alias.set_target("new-target");
        assert_eq!(alias.targets, vec!["new-target"]);
    }

    // ========================================================================
    // AliasType display
    // ========================================================================

    #[test]
    fn test_alias_type_display() {
        assert_eq!(AliasType::Write.to_string(), "write");
        assert_eq!(AliasType::Read.to_string(), "read");
    }

    // ========================================================================
    // update_write_target error path
    // ========================================================================

    #[tokio::test]
    async fn test_update_write_target_nonexistent() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        let result = manager
            .update_write_target("nonexistent", "new-target")
            .await;
        assert!(result.is_err());
    }

    // ========================================================================
    // resolve_write_target error path
    // ========================================================================

    #[tokio::test]
    async fn test_resolve_write_target_nonexistent() {
        let temp = TempDir::new().unwrap();
        let manager = AliasManager::new(temp.path()).await.unwrap();

        let result = manager.resolve_write_target("nonexistent").await;
        assert!(result.is_err());
    }
}
