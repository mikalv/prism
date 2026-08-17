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
    let has_ds = body.get("data_stream").is_some();
    let composed_of = body
        .get("composed_of")
        .and_then(|c| c.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    tracing::info!(
        %name, has_ds, composed_of,
        "PUT /_index_template (storing into composable store)"
    );
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

/// POST /_index_template/_simulate — preview the mappings/settings/aliases a
/// template body would produce.
///
/// Kibana plugins call this to validate mappings *before* installing a
/// template (`PUT /_index_template/{name}`); without it they log "Failed to
/// simulate index template mappings …; not applying mappings" and skip the
/// install. Prism stores templates as metadata only and never applies them to
/// collections, so simulation just echoes the `template` portion of the
/// request body. Registered on the `/_index_template/:name` route and branched
/// on `name == "_simulate"` because axum's matchit trie cannot co-register
/// `/_index_template/_simulate` as a separate static route. Any other
/// `POST /_index_template/{name}` is not part of the ES API and 405s.
pub async fn post_index_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    if name != "_simulate" {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }
    let components = state
        .templates
        .component
        .read()
        .expect("template store poisoned");
    let mappings = resolve_composed_mappings(&body, &components);
    let settings = body
        .pointer("/template/settings")
        .or_else(|| body.get("settings"))
        .cloned()
        .unwrap_or(json!({}));
    let aliases = body
        .pointer("/template/aliases")
        .or_else(|| body.get("aliases"))
        .cloned()
        .unwrap_or(json!({}));
    Ok(Json(json!({
        "template": { "mappings": mappings, "settings": settings, "aliases": aliases },
        "overlapping": []
    })))
}

/// POST /_index_template/{name}/_simulate — preview an already-stored template.
pub async fn simulate_named_index_template_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
    maybe_body: Option<Json<Value>>,
) -> Result<Json<Value>, StatusCode> {
    // Kibana's createOrUpdateIndexTemplate simulates with BOTH a name and
    // the full template body BEFORE installing it (pre-flight check). When a
    // body is present it wins over the stored copy — the stored copy is
    // either stale or absent at that point, and returning empty mappings
    // makes Kibana abort with "No mappings would be generated …".
    let body = match maybe_body {
        Some(Json(b)) if b.is_object() => Some(b),
        _ => state
            .templates
            .composable
            .read()
            .expect("template store poisoned")
            .get(&name)
            .cloned(),
    };
    match body {
        Some(b) => {
            let components = state
                .templates
                .component
                .read()
                .expect("template store poisoned");
            let mappings = resolve_composed_mappings(&b, &components);
            let settings = b.pointer("/template/settings").cloned().unwrap_or(json!({}));
            let aliases = b.pointer("/template/aliases").cloned().unwrap_or(json!({}));
            Ok(Json(json!({
                "template": { "mappings": mappings, "settings": settings, "aliases": aliases },
                "overlapping": []
            })))
        }
        None => {
            // Prism stores templates as metadata only and never applies them
            // to collections, so a simulate for an as-yet-unstored template
            // has nothing to resolve. Return a valid empty response instead
            // of 404: Kibana treats the 404 as a hard error. Some Kibana
            // flows (e.g. checking whether an installed template is current)
            // tolerate empty mappings here; the pre-install flow always sends
            // a body (handled above).
            tracing::debug!(%name, "simulate_named: template not stored, returning empty");
            Ok(Json(json!({ "template": {}, "overlapping": [] })))
        }
    }
}

// POST /_index_template/_simulate_index/{name} — Kibana's "create index"
// flow asks which templates would apply to a concrete index name. Prism
// applies templates loosely, so resolve any matching composable templates'
// mappings; an empty resolved template is a valid ES response when nothing
// matches and lets Kibana proceed with its own mappings.
pub async fn simulate_index_for_name_handler(
    State(state): State<EsCompatState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let composable = state.templates.composable.read().expect("template store poisoned");
    let mut merged_mappings = json!({});
    let mut overlapping = Vec::new();
    for (tpl_name, body) in composable.iter() {
        let patterns = body
            .get("index_patterns")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();
        let matched = patterns.iter().any(|p| {
            p.as_str()
                .map(|pat| pattern_matches(pat, &name))
                .unwrap_or(false)
        });
        if matched {
            overlapping.push(json!({ "name": tpl_name }));
            if let Some(m) = body.pointer("/template/mappings") {
                if let Some(props) = m.get("properties").and_then(|p| p.as_object()) {
                    if let Some(existing) = merged_mappings.get_mut("properties").and_then(|p| p.as_object_mut()) {
                        for (k, v) in props {
                            existing.insert(k.clone(), v.clone());
                        }
                    } else {
                        merged_mappings["properties"] = json!(props);
                    }
                }
            }
        }
    }
    Ok(Json(json!({
        "template": { "mappings": merged_mappings, "settings": {}, "aliases": {} },
        "overlapping": overlapping
    })))
}

/// Resolve an index-template body's effective `mappings` by merging, in ES
/// precedence order, the `composed_of` component templates (declaration
/// order; later wins for duplicate fields) followed by the index template's
/// own `template.mappings.properties` (or a bare `mappings.properties`).
///
/// Kibana's `createOrUpdateIndexTemplate` simulates a template and rejects
/// it with "No mappings would be generated … possibly due to failed/
/// misconfigured bootstrapping" unless the resolved mappings are non-empty —
/// many Kibana index templates carry NO own mappings and rely entirely on
/// `composed_of` component templates (e.g. `.alerts-*` → `.alerts-framework-
/// mappings`). Prism never applies templates to collections, but the simulate
/// response must still report the resolved mappings so Kibana proceeds.
fn resolve_composed_mappings(
    body: &Value,
    components: &HashMap<String, Value>,
) -> Value {
    let mut props = serde_json::Map::new();
    if let Some(composed) = body.get("composed_of").and_then(|c| c.as_array()) {
        for name in composed.iter().filter_map(|n| n.as_str()) {
            if let Some(cb) = components.get(name) {
                if let Some(cp) = cb
                    .pointer("/template/mappings/properties")
                    .and_then(|p| p.as_object())
                {
                    for (k, v) in cp {
                        props.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }
    // The index template's own mappings win (applied last, highest precedence).
    let own = body
        .pointer("/template/mappings/properties")
        .or_else(|| body.pointer("/mappings/properties"))
        .and_then(|p| p.as_object());
    if let Some(own) = own {
        for (k, v) in own {
            props.insert(k.clone(), v.clone());
        }
    }
    json!({ "properties": props })
}

/// Glob-style index-pattern match supporting leading `.` and trailing `*`.
fn pattern_matches(pattern: &str, name: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        name.starts_with(prefix)
    } else {
        pattern == name
    }
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

/// GET /_component_template — list all component templates.
/// Kibana verifies component-template installation (ECS mappings, data-stream
/// defaults, `composed_of` building blocks) by listing here; omitting it makes
/// the list return empty and Kibana repeatedly re-installs / fails to resolve
/// them during index-template simulation.
pub async fn get_all_component_templates_handler(
    State(state): State<EsCompatState>,
) -> Json<Value> {
    let map = state
        .templates
        .component
        .read()
        .expect("template store poisoned");
    let arr: Vec<Value> = map
        .iter()
        .map(|(n, b)| json!({ "name": n, "component_template": b }))
        .collect();
    Json(json!({ "component_templates": arr }))
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
