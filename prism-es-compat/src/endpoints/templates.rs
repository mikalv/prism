//! ES-compatible index-template endpoints: `_index_template` (composable),
//! `_component_template`, and `_template` (legacy).
//!
//! Kibana plugins register index templates at startup (observability, apm,
//! eventLog, taskManager, …). These define mappings/settings/aliases for
//! future indices matching a pattern. Prism collections are defined by schema
//! files, so templates are stored as metadata only — they are NOT applied to
//! collections at creation time. The store lets Kibana create, read, and
//! delete templates without error. Templates persist to
//! `{data_dir}/es-compat/templates.json` so they survive prism restarts.

use crate::endpoints::search::EsCompatState;
use crate::persist;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// In-memory store for the three ES template families.
#[derive(Clone, Default)]
pub struct TemplateStore {
    /// `/_index_template/{name}` (composable, ES ≥7.8)
    composable: Arc<RwLock<HashMap<String, Value>>>,
    /// `/_template/{name}` (legacy v1 templates)
    legacy: Arc<RwLock<HashMap<String, Value>>>,
    /// `/_component_template/{name}`
    component: Arc<RwLock<HashMap<String, Value>>>,
}

impl TemplateStore {
    fn put(family: &Arc<RwLock<HashMap<String, Value>>>, name: &str, body: Value) {
        family
            .write()
            .expect("template store poisoned")
            .insert(name.to_string(), body);
    }

    fn delete(family: &Arc<RwLock<HashMap<String, Value>>>, name: &str) -> bool {
        family
            .write()
            .expect("template store poisoned")
            .remove(name)
            .is_some()
    }

    fn contains(family: &Arc<RwLock<HashMap<String, Value>>>, name: &str) -> bool {
        family.read().expect("template store poisoned").contains_key(name)
    }

    /// GET wrapper shape: `{ <list_key>: [{ "name": name, <body_key>: body }] }`.
    fn get_one(
        family: &Arc<RwLock<HashMap<String, Value>>>,
        name: &str,
        list_key: &str,
        body_key: &str,
    ) -> Option<Value> {
        family
            .read()
            .expect("template store poisoned")
            .get(name)
            .map(|body| json!({ list_key: [{ "name": name, body_key: body }] }))
    }

    /// Snapshot all three families for persistence.
    fn snapshot(&self) -> Value {
        let snap = |f: &Arc<RwLock<HashMap<String, Value>>>| {
            f.read().expect("template store poisoned").clone()
        };
        json!({
            "composable": snap(&self.composable),
            "legacy": snap(&self.legacy),
            "component": snap(&self.component),
        })
    }

    /// Persist to `<dir>/templates.json`.
    pub fn persist_to(&self, dir: &std::path::Path) {
        persist::save_json(dir, "templates", &self.snapshot());
    }

    /// Load from `<dir>/templates.json` (empty store if absent/unparseable).
    pub fn load_from(dir: &std::path::Path) -> Self {
        let store = Self::default();
        if let Some(v) = persist::load_json::<Value>(dir, "templates") {
            if let Some(obj) = v.as_object() {
                for (family_name, target) in [
                    ("composable", &store.composable),
                    ("legacy", &store.legacy),
                    ("component", &store.component),
                ] {
                    if let Some(map) = obj.get(family_name).and_then(|m| m.as_object()) {
                        let mut guard = target.write().expect("template store poisoned");
                        for (k, val) in map {
                            guard.insert(k.clone(), val.clone());
                        }
                    }
                }
            }
            tracing::info!("Loaded persisted index templates from {}", dir.display());
        }
        store
    }
}

// ----------------------------------------------------------------------
// Composable index templates: /_index_template
// ----------------------------------------------------------------------

/// PUT /_index_template/{name}
pub async fn put_index_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    tracing::debug!(%name, "PUT /_index_template");
    TemplateStore::put(&state.templates.composable, &name, body);
    state.templates.persist_to(&state.data_dir);
    Json(json!({ "acknowledged": true }))
}

/// GET /_index_template/{name}
pub async fn get_index_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    match TemplateStore::get_one(
        &state.templates.composable,
        &name,
        "index_templates",
        "index_template",
    ) {
        Some(v) => Ok(Json(v)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// GET /_index_template (list all)
pub async fn get_all_index_templates_handler(
    State(state): State<EsCompatState>,
) -> Json<Value> {
    let map = state
        .templates
        .composable
        .read()
        .expect("template store poisoned");
    let arr: Vec<Value> = map
        .iter()
        .map(|(n, b)| json!({ "name": n, "index_template": b }))
        .collect();
    Json(json!({ "index_templates": arr }))
}

/// HEAD /_index_template/{name}
pub async fn head_index_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> StatusCode {
    if TemplateStore::contains(&state.templates.composable, &name) {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

/// DELETE /_index_template/{name}
pub async fn delete_index_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Json<Value> {
    TemplateStore::delete(&state.templates.composable, &name);
    state.templates.persist_to(&state.data_dir);
    Json(json!({ "acknowledged": true }))
}

// ----------------------------------------------------------------------
// Legacy v1 templates: /_template  (handlers exist; not routed — see router)
// ----------------------------------------------------------------------

pub async fn put_legacy_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    TemplateStore::put(&state.templates.legacy, &name, body);
    state.templates.persist_to(&state.data_dir);
    Json(json!({ "acknowledged": true }))
}

pub async fn get_legacy_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let map = state.templates.legacy.read().expect("template store poisoned");
    match map.get(&name) {
        Some(body) => Ok(Json(json!({ name: body }))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn head_legacy_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> StatusCode {
    if TemplateStore::contains(&state.templates.legacy, &name) {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

pub async fn delete_legacy_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Json<Value> {
    TemplateStore::delete(&state.templates.legacy, &name);
    state.templates.persist_to(&state.data_dir);
    Json(json!({ "acknowledged": true }))
}

// ----------------------------------------------------------------------
// Component templates: /_component_template
// ----------------------------------------------------------------------

/// PUT /_component_template/{name}
pub async fn put_component_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    TemplateStore::put(&state.templates.component, &name, body);
    state.templates.persist_to(&state.data_dir);
    Json(json!({ "acknowledged": true }))
}

/// GET /_component_template/{name}
pub async fn get_component_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    match TemplateStore::get_one(
        &state.templates.component,
        &name,
        "component_templates",
        "component_template",
    ) {
        Some(v) => Ok(Json(v)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// DELETE /_component_template/{name}
pub async fn delete_component_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Json<Value> {
    TemplateStore::delete(&state.templates.component, &name);
    state.templates.persist_to(&state.data_dir);
    Json(json!({ "acknowledged": true }))
}
