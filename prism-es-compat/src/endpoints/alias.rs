//! ES alias metadata store.
//!
//! The core `CollectionManager` carries only an `alias -> [indices]` map for
//! search/lookup resolution. ES aliases carry richer per-(alias, index)
//! metadata — most importantly `is_write_index`, but also `filter`, `routing`,
//! `is_hidden`, etc. This store layers the *full ES alias body* on top of the
//! core map so endpoints like `GET /{index}` and `_cat/aliases` can return the
//! metadata Kibana expects.
//!
//! Kibana's alerting/data-stream flow (`createConcreteWriteIndex`) creates an
//! index with `{ aliases: { <alias>: { is_write_index: true } } }`. If the
//! index already exists on a later boot, it fetches `GET /{index}` and checks
//! `aliases[<alias>].is_write_index`; without this store that field is always
//! absent, so Kibana throws "index already exists and is not the write index
//! for the alias".
//!
//! Persists to `{data_dir}/es-compat/alias_meta.json`.

use crate::persist;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

/// `alias name -> { index name -> ES alias body }`.
#[derive(Clone, Default)]
pub struct AliasStore {
    inner: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
}

impl AliasStore {
    /// Register/replace the alias body for an (alias, index) pair.
    pub fn add(&self, alias: &str, index: &str, body: Value) {
        self.inner
            .write()
            .expect("alias store poisoned")
            .entry(alias.to_string())
            .or_default()
            .insert(index.to_string(), body);
    }

    /// Remove the (alias, index) entry. Drops the alias entirely if empty.
    pub fn remove(&self, alias: &str, index: &str) {
        let mut g = self.inner.write().expect("alias store poisoned");
        if let Some(m) = g.get_mut(alias) {
            m.remove(index);
            if m.is_empty() {
                g.remove(alias);
            }
        }
    }

    /// All `(alias, body)` pairs that point at `index` — for `GET /{index}`.
    pub fn for_index(&self, index: &str) -> Vec<(String, Value)> {
        let g = self.inner.read().expect("alias store poisoned");
        let mut out = Vec::new();
        for (alias, targets) in g.iter() {
            if let Some(body) = targets.get(index) {
                out.push((alias.clone(), body.clone()));
            }
        }
        out
    }

    /// Full snapshot — `alias -> { index -> body }`.
    pub fn snapshot(&self) -> HashMap<String, HashMap<String, Value>> {
        self.inner.read().expect("alias store poisoned").clone()
    }

    /// Persist to `<dir>/alias_meta.json`.
    pub fn persist_to(&self, dir: &Path) {
        persist::save_json(dir, "alias_meta", &self.snapshot());
    }

    /// Load from `<dir>/alias_meta.json` (empty store if absent/unparseable).
    pub fn load_from(dir: &Path) -> Self {
        let store = Self::default();
        if let Some(map) =
            persist::load_json::<HashMap<String, HashMap<String, Value>>>(dir, "alias_meta")
        {
            *store.inner.write().expect("alias store poisoned") = map;
        }
        store
    }
}
