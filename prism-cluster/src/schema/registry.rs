//! Schema registry for tracking versions across the cluster
//!
//! The registry maintains the authoritative record of schema versions
//! for all collections and coordinates version updates.

use super::version::{detect_changes, SchemaVersion, VersionedSchema};
use super::PropagationStrategy;
use crate::error::ClusterError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Schema registry for tracking collection schemas
pub struct SchemaRegistry {
    /// Current schemas by collection
    schemas: Arc<RwLock<HashMap<String, VersionedSchema>>>,
    /// Schema history by collection (version -> schema)
    history: Arc<RwLock<HashMap<String, HashMap<u64, VersionedSchema>>>>,
    /// This node's ID
    node_id: String,
    /// Maximum history versions to keep per collection
    max_history: usize,
}

impl SchemaRegistry {
    /// Create a new schema registry
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            schemas: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(HashMap::new())),
            node_id: node_id.into(),
            max_history: 100,
        }
    }

    /// Set maximum history versions to keep
    pub fn with_max_history(mut self, max: usize) -> Self {
        self.max_history = max;
        self
    }

    /// Register a new schema version
    pub async fn register(
        &self,
        collection: &str,
        schema: serde_json::Value,
    ) -> Result<VersionedSchema, ClusterError> {
        let mut schemas = self.schemas.write().await;
        let mut history = self.history.write().await;

        // Determine the new version and changes
        let (version, changes) = if let Some(current) = schemas.get(collection) {
            let new_version = current.version.next();
            let changes = detect_changes(&current.schema, &schema, "");
            (new_version, changes)
        } else {
            (SchemaVersion::default(), Vec::new())
        };

        // Create versioned schema
        let versioned =
            VersionedSchema::new(collection, version, schema, &self.node_id).with_changes(changes);

        // Store in current schemas
        schemas.insert(collection.to_string(), versioned.clone());

        // Store in history
        let collection_history = history.entry(collection.to_string()).or_default();
        collection_history.insert(version.version(), versioned.clone());

        // Prune old history if needed
        if collection_history.len() > self.max_history {
            let min_version = collection_history.keys().copied().min().unwrap_or(0);
            collection_history.remove(&min_version);
        }

        info!(
            collection = collection,
            version = version.version(),
            "Registered new schema version"
        );

        Ok(versioned)
    }

    /// Get current schema for a collection
    pub async fn get(&self, collection: &str) -> Option<VersionedSchema> {
        self.schemas.read().await.get(collection).cloned()
    }

    /// Get current version for a collection
    pub async fn get_version(&self, collection: &str) -> Option<SchemaVersion> {
        self.schemas.read().await.get(collection).map(|s| s.version)
    }

    /// Get a specific version from history
    pub async fn get_version_from_history(
        &self,
        collection: &str,
        version: u64,
    ) -> Option<VersionedSchema> {
        self.history
            .read()
            .await
            .get(collection)
            .and_then(|h| h.get(&version).cloned())
    }

    /// List all collections with their current versions
    pub async fn list_collections(&self) -> Vec<(String, SchemaVersion)> {
        self.schemas
            .read()
            .await
            .iter()
            .map(|(name, schema)| (name.clone(), schema.version))
            .collect()
    }

    /// Get version history for a collection
    pub async fn get_history(&self, collection: &str) -> Vec<SchemaVersion> {
        self.history
            .read()
            .await
            .get(collection)
            .map(|h| {
                let mut versions: Vec<_> = h.keys().copied().collect();
                versions.sort();
                versions.into_iter().map(SchemaVersion::new).collect()
            })
            .unwrap_or_default()
    }

    /// Apply a schema from another node (for replication)
    pub async fn apply_remote_schema(
        &self,
        versioned: VersionedSchema,
    ) -> Result<bool, ClusterError> {
        let mut schemas = self.schemas.write().await;
        let mut history = self.history.write().await;

        let collection = &versioned.collection;

        // Check if we need to apply this version
        if let Some(current) = schemas.get(collection) {
            if !versioned.version.is_newer_than(&current.version) {
                debug!(
                    collection = collection,
                    remote_version = versioned.version.version(),
                    local_version = current.version.version(),
                    "Ignoring older schema version from remote"
                );
                return Ok(false);
            }
        }

        info!(
            collection = collection,
            version = versioned.version.version(),
            from = %versioned.created_by,
            "Applied remote schema version"
        );

        // Store in history
        let collection_history = history.entry(collection.to_string()).or_default();
        collection_history.insert(versioned.version.version(), versioned.clone());

        // Update current
        schemas.insert(collection.to_string(), versioned);

        Ok(true)
    }

    /// Check if a collection needs migration to a target version
    pub async fn needs_migration(&self, collection: &str, target: SchemaVersion) -> bool {
        self.schemas
            .read()
            .await
            .get(collection)
            .map(|s| target.is_newer_than(&s.version))
            .unwrap_or(true)
    }

    /// Determine propagation strategy based on schema changes
    pub fn determine_strategy(&self, versioned: &VersionedSchema) -> PropagationStrategy {
        if versioned.changes.is_empty() {
            // First version or no detectable changes
            PropagationStrategy::Immediate
        } else if versioned.has_breaking_changes() {
            // Breaking changes need coordinated migration
            PropagationStrategy::Versioned
        } else {
            // Additive changes can be applied immediately
            PropagationStrategy::Immediate
        }
    }

    /// Create a snapshot of the registry state
    pub async fn snapshot(&self) -> SchemaRegistrySnapshot {
        let schemas = self.schemas.read().await;
        SchemaRegistrySnapshot {
            schemas: schemas.clone(),
            node_id: self.node_id.clone(),
        }
    }

    /// Restore from a snapshot
    pub async fn restore(&self, snapshot: SchemaRegistrySnapshot) {
        let mut schemas = self.schemas.write().await;
        let mut history = self.history.write().await;

        for (collection, versioned) in snapshot.schemas {
            // Add to history
            let collection_history = history.entry(collection.clone()).or_default();
            collection_history.insert(versioned.version.version(), versioned.clone());

            // Update current if newer
            if let Some(current) = schemas.get(&collection) {
                if versioned.version.is_newer_than(&current.version) {
                    schemas.insert(collection, versioned);
                }
            } else {
                schemas.insert(collection, versioned);
            }
        }

        info!("Restored schema registry from snapshot");
    }

    /// Remove a collection's schema
    pub async fn remove(&self, collection: &str) -> Option<VersionedSchema> {
        let mut schemas = self.schemas.write().await;
        let removed = schemas.remove(collection);

        if removed.is_some() {
            warn!(collection = collection, "Removed collection schema");
        }

        removed
    }

    /// Clear all schemas (for testing)
    #[cfg(test)]
    pub async fn clear(&self) {
        self.schemas.write().await.clear();
        self.history.write().await.clear();
    }
}

/// Serializable snapshot of the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRegistrySnapshot {
    /// Current schemas by collection
    pub schemas: HashMap<String, VersionedSchema>,
    /// Node that created this snapshot
    pub node_id: String,
}

impl SchemaRegistrySnapshot {
    /// Get all collection names
    pub fn collections(&self) -> Vec<&str> {
        self.schemas.keys().map(|s| s.as_str()).collect()
    }

    /// Get schema for a collection
    pub fn get(&self, collection: &str) -> Option<&VersionedSchema> {
        self.schemas.get(collection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_register_first_schema() {
        let registry = SchemaRegistry::new("node-1");
        let schema = json!({
            "collection": "products",
            "backends": {"text": {"fields": ["title"]}}
        });

        let versioned = registry.register("products", schema).await.unwrap();

        assert_eq!(versioned.collection, "products");
        assert_eq!(versioned.version, SchemaVersion::new(1));
        assert!(versioned.changes.is_empty());
    }

    #[tokio::test]
    async fn test_register_updated_schema() {
        let registry = SchemaRegistry::new("node-1");

        // First version
        let v1 = json!({"fields": ["title"]});
        registry.register("products", v1).await.unwrap();

        // Second version with new field
        let v2 = json!({"fields": ["title", "description"]});
        let versioned = registry.register("products", v2).await.unwrap();

        assert_eq!(versioned.version, SchemaVersion::new(2));
        assert!(!versioned.changes.is_empty());
    }

    #[tokio::test]
    async fn test_get_schema() {
        let registry = SchemaRegistry::new("node-1");
        let schema = json!({"name": "test"});

        registry.register("test", schema.clone()).await.unwrap();

        let retrieved = registry.get("test").await.unwrap();
        assert_eq!(retrieved.schema, schema);
    }

    #[tokio::test]
    async fn test_apply_remote_schema() {
        let registry = SchemaRegistry::new("node-1");

        // Create a remote schema
        let remote = VersionedSchema::new(
            "products",
            SchemaVersion::new(5),
            json!({"remote": true}),
            "node-2",
        );

        let applied = registry.apply_remote_schema(remote).await.unwrap();
        assert!(applied);

        let current = registry.get("products").await.unwrap();
        assert_eq!(current.version, SchemaVersion::new(5));
    }

    #[tokio::test]
    async fn test_ignore_older_remote_schema() {
        let registry = SchemaRegistry::new("node-1");

        // Register local version 3
        let local = VersionedSchema::new(
            "products",
            SchemaVersion::new(3),
            json!({"local": true}),
            "node-1",
        );
        registry.apply_remote_schema(local).await.unwrap();

        // Try to apply older remote version
        let remote = VersionedSchema::new(
            "products",
            SchemaVersion::new(2),
            json!({"remote": true}),
            "node-2",
        );

        let applied = registry.apply_remote_schema(remote).await.unwrap();
        assert!(!applied);

        // Should still have version 3
        let current = registry.get("products").await.unwrap();
        assert_eq!(current.version, SchemaVersion::new(3));
    }

    #[tokio::test]
    async fn test_get_history() {
        let registry = SchemaRegistry::new("node-1");

        registry.register("test", json!({"v": 1})).await.unwrap();
        registry.register("test", json!({"v": 2})).await.unwrap();
        registry.register("test", json!({"v": 3})).await.unwrap();

        let history = registry.get_history("test").await;
        assert_eq!(history.len(), 3);
        assert_eq!(history[0], SchemaVersion::new(1));
        assert_eq!(history[2], SchemaVersion::new(3));
    }

    #[tokio::test]
    async fn test_snapshot_and_restore() {
        let registry1 = SchemaRegistry::new("node-1");
        registry1.register("col1", json!({"a": 1})).await.unwrap();
        registry1.register("col2", json!({"b": 2})).await.unwrap();

        let snapshot = registry1.snapshot().await;

        let registry2 = SchemaRegistry::new("node-2");
        registry2.restore(snapshot).await;

        assert!(registry2.get("col1").await.is_some());
        assert!(registry2.get("col2").await.is_some());
    }

    #[tokio::test]
    async fn test_determine_strategy() {
        let registry = SchemaRegistry::new("node-1");

        // First version - immediate
        let v1 = registry.register("test", json!({"a": 1})).await.unwrap();
        assert_eq!(
            registry.determine_strategy(&v1),
            PropagationStrategy::Immediate
        );

        // Additive change - immediate
        let v2 = registry
            .register("test", json!({"a": 1, "b": 2}))
            .await
            .unwrap();
        assert_eq!(
            registry.determine_strategy(&v2),
            PropagationStrategy::Immediate
        );

        // Breaking change (remove field) - versioned
        let v3 = registry.register("test", json!({"b": 2})).await.unwrap();
        assert_eq!(
            registry.determine_strategy(&v3),
            PropagationStrategy::Versioned
        );
    }

    // --- get_version ---

    #[tokio::test]
    async fn test_get_version() {
        let registry = SchemaRegistry::new("node-1");

        assert!(registry.get_version("nonexistent").await.is_none());

        registry.register("test", json!({"a": 1})).await.unwrap();
        let version = registry.get_version("test").await.unwrap();
        assert_eq!(version, SchemaVersion::new(1));
    }

    // --- get nonexistent ---

    #[tokio::test]
    async fn test_get_nonexistent_collection() {
        let registry = SchemaRegistry::new("node-1");
        assert!(registry.get("nonexistent").await.is_none());
    }

    // --- get_version_from_history ---

    #[tokio::test]
    async fn test_get_version_from_history() {
        let registry = SchemaRegistry::new("node-1");

        registry.register("test", json!({"v": 1})).await.unwrap();
        registry.register("test", json!({"v": 2})).await.unwrap();
        registry.register("test", json!({"v": 3})).await.unwrap();

        let v1 = registry.get_version_from_history("test", 1).await;
        assert!(v1.is_some());
        assert_eq!(v1.unwrap().version, SchemaVersion::new(1));

        let v2 = registry.get_version_from_history("test", 2).await;
        assert!(v2.is_some());

        let v99 = registry.get_version_from_history("test", 99).await;
        assert!(v99.is_none());
    }

    #[tokio::test]
    async fn test_get_version_from_history_nonexistent_collection() {
        let registry = SchemaRegistry::new("node-1");
        assert!(
            registry
                .get_version_from_history("nonexistent", 1)
                .await
                .is_none()
        );
    }

    // --- list_collections ---

    #[tokio::test]
    async fn test_list_collections() {
        let registry = SchemaRegistry::new("node-1");

        let collections = registry.list_collections().await;
        assert!(collections.is_empty());

        registry.register("products", json!({"a": 1})).await.unwrap();
        registry.register("orders", json!({"b": 2})).await.unwrap();

        let collections = registry.list_collections().await;
        assert_eq!(collections.len(), 2);

        let names: Vec<_> = collections.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"products"));
        assert!(names.contains(&"orders"));
    }

    // --- get_history ---

    #[tokio::test]
    async fn test_get_history_nonexistent() {
        let registry = SchemaRegistry::new("node-1");
        let history = registry.get_history("nonexistent").await;
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_get_history_sorted() {
        let registry = SchemaRegistry::new("node-1");

        registry.register("test", json!({"v": 1})).await.unwrap();
        registry.register("test", json!({"v": 2})).await.unwrap();
        registry.register("test", json!({"v": 3})).await.unwrap();

        let history = registry.get_history("test").await;
        assert_eq!(history.len(), 3);
        // Should be sorted ascending
        assert!(history[0] < history[1]);
        assert!(history[1] < history[2]);
    }

    // --- needs_migration ---

    #[tokio::test]
    async fn test_needs_migration_nonexistent() {
        let registry = SchemaRegistry::new("node-1");
        // Nonexistent collection always needs migration
        assert!(registry.needs_migration("test", SchemaVersion::new(1)).await);
    }

    #[tokio::test]
    async fn test_needs_migration_current() {
        let registry = SchemaRegistry::new("node-1");
        registry.register("test", json!({"a": 1})).await.unwrap();

        // Current version = 1, target = 1, no migration needed
        assert!(!registry.needs_migration("test", SchemaVersion::new(1)).await);
        // Target = 2, needs migration
        assert!(registry.needs_migration("test", SchemaVersion::new(2)).await);
    }

    // --- remove ---

    #[tokio::test]
    async fn test_remove_collection() {
        let registry = SchemaRegistry::new("node-1");
        registry.register("test", json!({"a": 1})).await.unwrap();
        assert!(registry.get("test").await.is_some());

        let removed = registry.remove("test").await;
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().collection, "test");

        assert!(registry.get("test").await.is_none());
    }

    #[tokio::test]
    async fn test_remove_nonexistent() {
        let registry = SchemaRegistry::new("node-1");
        let removed = registry.remove("nonexistent").await;
        assert!(removed.is_none());
    }

    // --- apply_remote_schema same version ---

    #[tokio::test]
    async fn test_apply_remote_schema_same_version() {
        let registry = SchemaRegistry::new("node-1");

        let v1 = VersionedSchema::new(
            "test",
            SchemaVersion::new(3),
            json!({"a": 1}),
            "node-2",
        );
        registry.apply_remote_schema(v1).await.unwrap();

        // Same version should be rejected
        let v1_again = VersionedSchema::new(
            "test",
            SchemaVersion::new(3),
            json!({"a": 2}),
            "node-3",
        );
        let applied = registry.apply_remote_schema(v1_again).await.unwrap();
        assert!(!applied);
    }

    // --- with_max_history ---

    #[tokio::test]
    async fn test_max_history_pruning() {
        let registry = SchemaRegistry::new("node-1").with_max_history(3);

        for i in 1..=5 {
            registry
                .register("test", json!({"version": i}))
                .await
                .unwrap();
        }

        let history = registry.get_history("test").await;
        // Should have pruned oldest versions, keeping max 3
        assert!(history.len() <= 3);
    }

    // --- clear ---

    #[tokio::test]
    async fn test_clear() {
        let registry = SchemaRegistry::new("node-1");
        registry.register("col1", json!({"a": 1})).await.unwrap();
        registry.register("col2", json!({"b": 2})).await.unwrap();

        registry.clear().await;
        assert!(registry.list_collections().await.is_empty());
    }

    // --- SchemaRegistrySnapshot ---

    #[tokio::test]
    async fn test_snapshot_collections() {
        let registry = SchemaRegistry::new("node-1");
        registry.register("col1", json!({"a": 1})).await.unwrap();
        registry.register("col2", json!({"b": 2})).await.unwrap();

        let snapshot = registry.snapshot().await;
        let collections = snapshot.collections();
        assert_eq!(collections.len(), 2);
        assert_eq!(snapshot.node_id, "node-1");

        assert!(snapshot.get("col1").is_some());
        assert!(snapshot.get("col2").is_some());
        assert!(snapshot.get("nonexistent").is_none());
    }

    // --- restore from snapshot with newer version ---

    #[tokio::test]
    async fn test_restore_newer_version() {
        let registry1 = SchemaRegistry::new("node-1");
        registry1.register("col1", json!({"v": 1})).await.unwrap();
        registry1.register("col1", json!({"v": 2})).await.unwrap();

        let snapshot = registry1.snapshot().await;

        let registry2 = SchemaRegistry::new("node-2");
        // Pre-register a v1 schema
        registry2.register("col1", json!({"v": 1})).await.unwrap();

        // Restore should overwrite with newer version from snapshot (v2)
        registry2.restore(snapshot).await;

        let current = registry2.get("col1").await.unwrap();
        assert_eq!(current.version, SchemaVersion::new(2));
    }

    // --- restore doesn't downgrade ---

    #[tokio::test]
    async fn test_restore_does_not_downgrade() {
        let registry = SchemaRegistry::new("node-1");
        registry.register("col1", json!({"v": 1})).await.unwrap();
        registry.register("col1", json!({"v": 2})).await.unwrap();
        registry.register("col1", json!({"v": 3})).await.unwrap();

        // Create a snapshot at v1 from another registry
        let old_registry = SchemaRegistry::new("node-2");
        old_registry
            .register("col1", json!({"v": 1}))
            .await
            .unwrap();
        let old_snapshot = old_registry.snapshot().await;

        // Restoring old snapshot should NOT downgrade from v3
        registry.restore(old_snapshot).await;
        let current = registry.get("col1").await.unwrap();
        assert_eq!(current.version, SchemaVersion::new(3));
    }
}
