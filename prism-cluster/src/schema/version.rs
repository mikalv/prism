//! Schema versioning types for distributed Prism
//!
//! Provides versioned schema tracking with change detection and migration support.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// A schema version identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SchemaVersion(pub u64);

impl SchemaVersion {
    /// Create a new schema version
    pub fn new(version: u64) -> Self {
        Self(version)
    }

    /// Get the version number
    pub fn version(&self) -> u64 {
        self.0
    }

    /// Increment to next version
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }

    /// Check if this version is newer than another
    pub fn is_newer_than(&self, other: &SchemaVersion) -> bool {
        self.0 > other.0
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        Self(1)
    }
}

impl std::fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// Type of schema change
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    /// New field added (backward compatible)
    FieldAdded,
    /// Field removed (breaking)
    FieldRemoved,
    /// Field type changed (breaking)
    FieldTypeChanged,
    /// Field made required (breaking)
    FieldMadeRequired,
    /// Field made optional (backward compatible)
    FieldMadeOptional,
    /// New index added
    IndexAdded,
    /// Index removed
    IndexRemoved,
    /// Index settings changed
    IndexSettingsChanged,
    /// Backend configuration changed
    BackendConfigChanged,
    /// Collection settings changed
    CollectionSettingsChanged,
}

impl ChangeType {
    /// Check if this change type is breaking (requires coordinated migration)
    pub fn is_breaking(&self) -> bool {
        matches!(
            self,
            ChangeType::FieldRemoved
                | ChangeType::FieldTypeChanged
                | ChangeType::FieldMadeRequired
                | ChangeType::IndexRemoved
        )
    }

    /// Check if this change is additive (can be applied immediately)
    pub fn is_additive(&self) -> bool {
        matches!(
            self,
            ChangeType::FieldAdded
                | ChangeType::FieldMadeOptional
                | ChangeType::IndexAdded
        )
    }
}

/// A specific schema change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaChange {
    /// Type of change
    pub change_type: ChangeType,
    /// Path to the changed element (e.g., "backends.text.fields.title")
    pub path: String,
    /// Previous value (serialized)
    pub old_value: Option<serde_json::Value>,
    /// New value (serialized)
    pub new_value: Option<serde_json::Value>,
    /// Human-readable description
    pub description: String,
}

impl SchemaChange {
    /// Create a new schema change
    pub fn new(
        change_type: ChangeType,
        path: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            change_type,
            path: path.into(),
            old_value: None,
            new_value: None,
            description: description.into(),
        }
    }

    /// Set the old value
    pub fn with_old_value(mut self, value: serde_json::Value) -> Self {
        self.old_value = Some(value);
        self
    }

    /// Set the new value
    pub fn with_new_value(mut self, value: serde_json::Value) -> Self {
        self.new_value = Some(value);
        self
    }

    /// Check if this is a breaking change
    pub fn is_breaking(&self) -> bool {
        self.change_type.is_breaking()
    }
}

/// A versioned schema with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedSchema {
    /// Collection name
    pub collection: String,
    /// Schema version
    pub version: SchemaVersion,
    /// The actual schema content (serialized CollectionSchema)
    pub schema: serde_json::Value,
    /// When this version was created (unix timestamp ms)
    pub created_at: u64,
    /// Node that created this version
    pub created_by: String,
    /// Changes from previous version
    pub changes: Vec<SchemaChange>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl VersionedSchema {
    /// Create a new versioned schema
    pub fn new(
        collection: impl Into<String>,
        version: SchemaVersion,
        schema: serde_json::Value,
        created_by: impl Into<String>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            collection: collection.into(),
            version,
            schema,
            created_at: now,
            created_by: created_by.into(),
            changes: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add changes from previous version
    pub fn with_changes(mut self, changes: Vec<SchemaChange>) -> Self {
        self.changes = changes;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Check if this version has breaking changes
    pub fn has_breaking_changes(&self) -> bool {
        self.changes.iter().any(|c| c.is_breaking())
    }

    /// Get all breaking changes
    pub fn breaking_changes(&self) -> Vec<&SchemaChange> {
        self.changes.iter().filter(|c| c.is_breaking()).collect()
    }

    /// Get all additive changes
    pub fn additive_changes(&self) -> Vec<&SchemaChange> {
        self.changes
            .iter()
            .filter(|c| c.change_type.is_additive())
            .collect()
    }
}

/// Compare two schemas and detect changes
pub fn detect_changes(
    old_schema: &serde_json::Value,
    new_schema: &serde_json::Value,
    path: &str,
) -> Vec<SchemaChange> {
    let mut changes = Vec::new();
    detect_changes_recursive(old_schema, new_schema, path, &mut changes);
    changes
}

fn detect_changes_recursive(
    old: &serde_json::Value,
    new: &serde_json::Value,
    path: &str,
    changes: &mut Vec<SchemaChange>,
) {
    match (old, new) {
        (serde_json::Value::Object(old_map), serde_json::Value::Object(new_map)) => {
            // Check for removed fields
            for key in old_map.keys() {
                if !new_map.contains_key(key) {
                    let field_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", path, key)
                    };
                    changes.push(
                        SchemaChange::new(
                            ChangeType::FieldRemoved,
                            &field_path,
                            format!("Field '{}' was removed", key),
                        )
                        .with_old_value(old_map[key].clone()),
                    );
                }
            }

            // Check for added or changed fields
            for (key, new_value) in new_map {
                let field_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };

                match old_map.get(key) {
                    None => {
                        changes.push(
                            SchemaChange::new(
                                ChangeType::FieldAdded,
                                &field_path,
                                format!("Field '{}' was added", key),
                            )
                            .with_new_value(new_value.clone()),
                        );
                    }
                    Some(old_value) => {
                        if old_value != new_value {
                            // Recurse into nested objects
                            if old_value.is_object() && new_value.is_object() {
                                detect_changes_recursive(old_value, new_value, &field_path, changes);
                            } else {
                                // Type or value changed
                                changes.push(
                                    SchemaChange::new(
                                        ChangeType::FieldTypeChanged,
                                        &field_path,
                                        format!("Field '{}' was changed", key),
                                    )
                                    .with_old_value(old_value.clone())
                                    .with_new_value(new_value.clone()),
                                );
                            }
                        }
                    }
                }
            }
        }
        (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr)) => {
            if old_arr != new_arr {
                changes.push(
                    SchemaChange::new(
                        ChangeType::FieldTypeChanged,
                        path,
                        "Array contents changed",
                    )
                    .with_old_value(old.clone())
                    .with_new_value(new.clone()),
                );
            }
        }
        _ => {
            if old != new {
                changes.push(
                    SchemaChange::new(ChangeType::FieldTypeChanged, path, "Value changed")
                        .with_old_value(old.clone())
                        .with_new_value(new.clone()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_schema_version_ordering() {
        let v1 = SchemaVersion::new(1);
        let v2 = SchemaVersion::new(2);

        assert!(v2.is_newer_than(&v1));
        assert!(!v1.is_newer_than(&v2));
        assert_eq!(v1.next(), v2);
    }

    #[test]
    fn test_change_type_breaking() {
        assert!(ChangeType::FieldRemoved.is_breaking());
        assert!(ChangeType::FieldTypeChanged.is_breaking());
        assert!(ChangeType::FieldMadeRequired.is_breaking());
        assert!(!ChangeType::FieldAdded.is_breaking());
        assert!(!ChangeType::FieldMadeOptional.is_breaking());
    }

    #[test]
    fn test_detect_added_field() {
        let old = json!({"name": "test"});
        let new = json!({"name": "test", "description": "new field"});

        let changes = detect_changes(&old, &new, "");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change_type, ChangeType::FieldAdded);
        assert_eq!(changes[0].path, "description");
    }

    #[test]
    fn test_detect_removed_field() {
        let old = json!({"name": "test", "description": "old field"});
        let new = json!({"name": "test"});

        let changes = detect_changes(&old, &new, "");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change_type, ChangeType::FieldRemoved);
        assert!(changes[0].is_breaking());
    }

    #[test]
    fn test_detect_nested_changes() {
        let old = json!({
            "backends": {
                "text": {
                    "fields": ["title"]
                }
            }
        });
        let new = json!({
            "backends": {
                "text": {
                    "fields": ["title", "description"]
                }
            }
        });

        let changes = detect_changes(&old, &new, "");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "backends.text.fields");
    }

    #[test]
    fn test_versioned_schema() {
        let schema = json!({"collection": "products"});
        let versioned = VersionedSchema::new("products", SchemaVersion::new(1), schema, "node-1")
            .with_changes(vec![SchemaChange::new(
                ChangeType::FieldAdded,
                "description",
                "Added description field",
            )])
            .with_metadata("author", "admin");

        assert_eq!(versioned.collection, "products");
        assert_eq!(versioned.version.version(), 1);
        assert!(!versioned.has_breaking_changes());
        assert_eq!(versioned.metadata.get("author"), Some(&"admin".to_string()));
    }
}
