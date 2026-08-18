//! Output rendering: human tables (default) or verbatim server JSON.
use serde_json::Value;
use std::fmt::Write as _;

pub struct Output { json: bool }

impl Output {
    pub fn new(json: bool) -> Self { Self { json } }
    pub fn is_json(&self) -> bool { self.json }
    pub fn raw(&self, v: &Value) {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    }
    pub fn line(&self, s: &str) { println!("{}", s); }
}

pub struct CollectionRow { pub name: String, pub docs: usize, pub bytes: u64 }

pub(crate) fn human_bytes(n: u64) -> String {
    const K: u64 = 1024;
    if n < K { return n.to_string(); }
    let (v, unit) = if n < K * K { (n as f64 / K as f64, "K") }
        else if n < K * K * K { (n as f64 / (K * K) as f64, "M") }
        else { (n as f64 / (K * K * K) as f64, "G") };
    format!("{:.1}{}", v, unit)
}

fn snippet(fields: &serde_json::Map<String, Value>) -> String {
    for key in ["title", "name", "url"] {
        if let Some(Value::String(s)) = fields.get(key) {
            let t: String = s.chars().take(80).collect();
            return t;
        }
    }
    for (_k, v) in fields {
        if let Value::String(s) = v {
            let t: String = s.chars().take(80).collect();
            return t;
        }
    }
    String::new()
}

pub(crate) fn render_search(v: &Value) -> String {
    let mut out = String::new();
    let total = v.get("total").and_then(Value::as_u64).unwrap_or(0);
    let _ = writeln!(out, "total: {}", total);
    if let Some(results) = v.get("results").and_then(Value::as_array) {
        for (i, r) in results.iter().enumerate() {
            let id = r.get("id").and_then(Value::as_str).unwrap_or("?");
            let score = r.get("score").and_then(Value::as_f64).unwrap_or(0.0);
            let fields = r.get("fields").and_then(Value::as_object);
            let snip = fields.map(snippet).unwrap_or_default();
            let _ = writeln!(out, "{:>3}. {:<7.2}  {:<20}  {}", i + 1, score, id, snip);
        }
    }
    out
}

pub(crate) fn render_collections(rows: &[CollectionRow]) -> String {
    let mut out = String::from("NAME  DOCS  SIZE\n");
    for r in rows {
        let _ = writeln!(out, "{}  {}  {}", r.name, r.docs, human_bytes(r.bytes));
    }
    out
}

pub(crate) fn render_document(v: &Value) -> String {
    if v.is_null() { return "document not found".to_string(); }
    let mut out = String::new();
    if let Some(id) = v.get("id").and_then(Value::as_str) {
        let _ = writeln!(out, "id: {}", id);
    }
    if let Some(fields) = v.get("fields").and_then(Value::as_object) {
        let mut keys: Vec<&String> = fields.keys().collect();
        keys.sort();
        for k in keys {
            let _ = writeln!(out, "{}: {}", k, serde_json::to_string(&fields[k]).unwrap_or_default());
        }
    }
    out
}

pub(crate) fn render_graph_path(v: &Value) -> String {
    match v.get("path").and_then(Value::as_array) {
        Some(nodes) if !nodes.is_empty() => {
            let chain: Vec<&str> = nodes.iter().filter_map(Value::as_str).collect();
            format!("{} ({} hops)", chain.join(" -> "), chain.len().saturating_sub(1))
        }
        _ => "no path found".to_string(),
    }
}

pub fn print_collections(out: &Output, rows: &[CollectionRow]) {
    if out.is_json() {
        let v = serde_json::json!({ "collections": rows.iter()
            .map(|r| serde_json::json!({"name": r.name, "docs": r.docs, "bytes": r.bytes}))
            .collect::<Vec<_>>() });
        out.raw(&v);
    } else {
        print!("{}", render_collections(rows));
    }
}

pub fn print_search(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v) } else { print!("{}", render_search(v)) }
}

pub fn print_document(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v) } else { print!("{}", render_document(v)) }
}

pub fn print_reindex(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v); return; }
    let mut s = String::new();
    if let Some(colls) = v.get("collections").and_then(Value::as_array) {
        for c in colls {
            let _ = writeln!(s, "{}: reembedded={}, skipped={}",
                c.get("collection").and_then(Value::as_str).unwrap_or("?"),
                c.get("reembedded").and_then(Value::as_u64).unwrap_or(0),
                c.get("skipped").and_then(Value::as_u64).unwrap_or(0));
        }
    }
    let _ = writeln!(s, "total: reembedded={}, skipped={}",
        v.get("total_reembedded").and_then(Value::as_u64).unwrap_or(0),
        v.get("total_skipped").and_then(Value::as_u64).unwrap_or(0));
    print!("{}", s);
}

pub fn print_schema(out: &Output, name: &str, v: &Value) {
    if out.is_json() { out.raw(v); return; }
    println!("schema for '{}':", name);
    if let Ok(y) = serde_yaml::to_string(v) { print!("{}", y) }
}

pub fn print_lint(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v); return; }
    let obj = v.as_object().cloned().unwrap_or_default();
    if obj.is_empty() { println!("no issues"); return; }
    for (name, issues) in &obj {
        println!("{}:", name);
        if let Some(list) = issues.as_array() {
            for i in list { println!("  - {}", i); }
        }
    }
}

pub fn print_backup(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v); return; }
    let ok = v.get("success").and_then(Value::as_bool).unwrap_or(false);
    let coll = v.get("collection").and_then(Value::as_str).unwrap_or("?");
    if ok {
        match v.get("size_bytes") {
            Some(s) => println!("backup of '{}' ok ({} bytes)", coll, s.as_u64().unwrap_or(0)),
            None => println!("restore of '{}' ok ({} files, {} bytes)", coll,
                v.get("files_extracted").and_then(Value::as_u64).unwrap_or(0),
                v.get("bytes_extracted").and_then(Value::as_u64).unwrap_or(0)),
        }
    } else {
        println!("FAILED: {}", v.get("error").and_then(Value::as_str).unwrap_or("unknown error"));
    }
}

pub fn print_graph_bfs(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v); return; }
    let nodes: Vec<&str> = v.get("nodes").and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    println!("{} nodes reached:", nodes.len());
    for n in nodes { println!("  {}", n); }
}

pub fn print_graph_path(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v) } else { println!("{}", render_graph_path(v)) }
}

pub fn print_graph_stats(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v); return; }
    if let Some(obj) = v.as_object() {
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        for k in keys { println!("{}: {}", k, serde_json::to_string(&obj[k]).unwrap_or_default()); }
    }
}

pub fn print_suggest(out: &Output, v: &Value) {
    if out.is_json() { out.raw(v); return; }
    if let Some(list) = v.get("suggestions").and_then(Value::as_array) {
        for s in list { println!("{}", serde_json::to_string(s).unwrap_or_default()); }
    }
    if let Some(dym) = v.get("did_you_mean").and_then(Value::as_str) {
        println!("did you mean: {}", dym);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(512), "512");
        assert_eq!(human_bytes(2048), "2.0K");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0M");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0G");
    }

    #[test]
    fn collections_table_lists_names() {
        let rows = vec![CollectionRow { name: "idx_web".into(), docs: 42, bytes: 2048 }];
        // capture via format-like helper: render to String
        assert!(render_collections(&rows).contains("idx_web"));
        assert!(render_collections(&rows).contains("2.0K"));
    }

    #[test]
    fn search_table_shows_score_and_snippet() {
        let v: Value = serde_json::json!({
            "total": 1,
            "results": [{ "id": "doc1", "score": 1.25,
                          "fields": { "title": "Privacy Guides", "url": "https://privacyguides.org" } }]
        });
        let s = render_search(&v);
        assert!(s.contains("doc1") && s.contains("1.25"));
        assert!(s.contains("Privacy Guides"));
    }

    #[test]
    fn search_snippet_falls_back_to_any_text_field() {
        let v: Value = serde_json::json!({
            "total": 1,
            "results": [{ "id": "d", "score": 0.5, "fields": { "body": "some long body text" } }]
        });
        assert!(render_search(&v).contains("some long body"));
    }

    #[test]
    fn graph_path_renders_chain() {
        let v: Value = serde_json::json!({"path": ["a","b","c"], "length": 2});
        assert!(render_graph_path(&v).contains("a -> b -> c"));
        let none: Value = serde_json::json!({"path": null, "length": null});
        assert!(render_graph_path(&none).contains("no path"));
    }

    #[test]
    fn document_null_prints_not_found() {
        assert!(render_document(&Value::Null).contains("not found"));
    }
}
