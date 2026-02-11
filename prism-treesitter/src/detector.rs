//! Language auto-detection from file content.
//!
//! Uses shebangs and keyword heuristics to identify the programming language.

use crate::Language;

/// Detect language from a file extension.
pub fn language_from_extension(ext: &str) -> Option<Language> {
    match ext.to_lowercase().as_str() {
        #[cfg(feature = "rust")]
        "rs" => Some(Language::Rust),
        #[cfg(feature = "python")]
        "py" | "pyw" | "pyi" => Some(Language::Python),
        #[cfg(feature = "javascript")]
        "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
        #[cfg(feature = "typescript")]
        "ts" | "tsx" | "mts" | "cts" => Some(Language::TypeScript),
        #[cfg(feature = "go")]
        "go" => Some(Language::Go),
        #[cfg(feature = "c")]
        "c" | "h" => Some(Language::C),
        #[cfg(feature = "cpp")]
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some(Language::Cpp),
        #[cfg(feature = "ruby")]
        "rb" | "rake" | "gemspec" => Some(Language::Ruby),
        #[cfg(feature = "elixir")]
        "ex" | "exs" => Some(Language::Elixir),
        #[cfg(feature = "erlang")]
        "erl" | "hrl" => Some(Language::Erlang),
        #[cfg(feature = "bash")]
        "sh" | "bash" | "zsh" => Some(Language::Bash),
        #[cfg(feature = "yaml")]
        "yaml" | "yml" => Some(Language::Yaml),
        #[cfg(feature = "toml")]
        "toml" => Some(Language::Toml),
        #[cfg(feature = "json")]
        "json" | "jsonl" => Some(Language::Json),
        #[cfg(feature = "html")]
        "html" | "htm" => Some(Language::Html),
        #[cfg(feature = "sql")]
        "sql" | "ddl" | "dml" => Some(Language::Sql),
        _ => None,
    }
}

/// Detect language from file content using shebangs and keyword heuristics.
pub fn language_from_content(text: &str) -> Option<Language> {
    // Check shebang line
    if let Some(lang) = detect_from_shebang(text) {
        return Some(lang);
    }

    // Keyword heuristics (score-based)
    detect_from_keywords(text)
}

fn detect_from_shebang(text: &str) -> Option<Language> {
    let first_line = text.lines().next()?;
    if !first_line.starts_with("#!") {
        return None;
    }
    let shebang = first_line.to_lowercase();

    #[cfg(feature = "python")]
    if shebang.contains("python") {
        return Some(Language::Python);
    }

    #[cfg(feature = "ruby")]
    if shebang.contains("ruby") {
        return Some(Language::Ruby);
    }

    #[cfg(feature = "bash")]
    if shebang.contains("bash") || shebang.contains("/sh") {
        return Some(Language::Bash);
    }

    #[cfg(feature = "elixir")]
    if shebang.contains("elixir") {
        return Some(Language::Elixir);
    }

    #[cfg(feature = "erlang")]
    if shebang.contains("escript") {
        return Some(Language::Erlang);
    }

    None
}

fn detect_from_keywords(text: &str) -> Option<Language> {
    // Take a sample of the text for analysis (first 4KB)
    let sample = if text.len() > 4096 {
        &text[..4096]
    } else {
        text
    };

    let mut best: Option<(Language, u32)> = None;

    let mut check = |lang: Language, score: u32| {
        if score > 0 {
            if let Some((_, best_score)) = best {
                if score > best_score {
                    best = Some((lang, score));
                }
            } else {
                best = Some((lang, score));
            }
        }
    };

    #[cfg(feature = "rust")]
    {
        let mut score = 0u32;
        if sample.contains("fn ") {
            score += 2;
        }
        if sample.contains("let ") || sample.contains("let mut ") {
            score += 2;
        }
        if sample.contains("impl ") {
            score += 3;
        }
        if sample.contains("pub fn ") || sample.contains("pub struct ") {
            score += 3;
        }
        if sample.contains("use std::") || sample.contains("use crate::") {
            score += 3;
        }
        if sample.contains("-> Result<") || sample.contains("-> Option<") {
            score += 2;
        }
        check(Language::Rust, score);
    }

    #[cfg(feature = "python")]
    {
        let mut score = 0u32;
        if sample.contains("def ") {
            score += 2;
        }
        if sample.contains("import ") {
            score += 1;
        }
        if sample.contains("class ") && sample.contains("self") {
            score += 3;
        }
        if sample.contains("__init__") {
            score += 3;
        }
        if sample.contains("if __name__") {
            score += 4;
        }
        check(Language::Python, score);
    }

    #[cfg(feature = "javascript")]
    {
        let mut score = 0u32;
        if sample.contains("function ") {
            score += 1;
        }
        if sample.contains("const ") || sample.contains("let ") {
            score += 1;
        }
        if sample.contains("require(") {
            score += 3;
        }
        if sample.contains("module.exports") {
            score += 4;
        }
        if sample.contains("console.log") {
            score += 2;
        }
        if sample.contains("=> {") || sample.contains("=> (") {
            score += 1;
        }
        check(Language::JavaScript, score);
    }

    #[cfg(feature = "typescript")]
    {
        let mut score = 0u32;
        if sample.contains("interface ") {
            score += 3;
        }
        if sample.contains(": string")
            || sample.contains(": number")
            || sample.contains(": boolean")
        {
            score += 3;
        }
        if sample.contains("export type ") || sample.contains("export interface ") {
            score += 4;
        }
        if sample.contains("<T>") || sample.contains("<T,") {
            score += 2;
        }
        check(Language::TypeScript, score);
    }

    #[cfg(feature = "go")]
    {
        let mut score = 0u32;
        if sample.contains("func ") {
            score += 2;
        }
        if sample.contains("package ") {
            score += 3;
        }
        if sample.contains("go func") || sample.contains("go ") {
            score += 2;
        }
        if sample.contains("fmt.") {
            score += 3;
        }
        if sample.contains(":= ") {
            score += 2;
        }
        check(Language::Go, score);
    }

    #[cfg(feature = "c")]
    {
        let mut score = 0u32;
        if sample.contains("#include") {
            score += 2;
        }
        if sample.contains("int main(") {
            score += 3;
        }
        if sample.contains("printf(") || sample.contains("malloc(") {
            score += 2;
        }
        if sample.contains("void ") {
            score += 1;
        }
        check(Language::C, score);
    }

    #[cfg(feature = "cpp")]
    {
        let mut score = 0u32;
        if sample.contains("#include") {
            score += 1;
        }
        if sample.contains("std::") {
            score += 3;
        }
        if sample.contains("class ") && sample.contains("public:") {
            score += 4;
        }
        if sample.contains("template<") || sample.contains("template <") {
            score += 4;
        }
        if sample.contains("namespace ") {
            score += 3;
        }
        check(Language::Cpp, score);
    }

    #[cfg(feature = "ruby")]
    {
        let mut score = 0u32;
        if sample.contains("def ") && sample.contains("end") {
            score += 2;
        }
        if sample.contains("require ") || sample.contains("require_relative") {
            score += 3;
        }
        if sample.contains("attr_accessor") || sample.contains("attr_reader") {
            score += 4;
        }
        if sample.contains("puts ") || sample.contains(".each do") {
            score += 2;
        }
        check(Language::Ruby, score);
    }

    #[cfg(feature = "elixir")]
    {
        let mut score = 0u32;
        if sample.contains("defmodule ") {
            score += 5;
        }
        if sample.contains("defp ") || sample.contains("def ") {
            score += 1;
        }
        if sample.contains("|> ") {
            score += 3;
        }
        if sample.contains("@spec") || sample.contains("@doc") {
            score += 3;
        }
        check(Language::Elixir, score);
    }

    #[cfg(feature = "erlang")]
    {
        let mut score = 0u32;
        if sample.contains("-module(") {
            score += 5;
        }
        if sample.contains("-export(") {
            score += 4;
        }
        if sample.contains("->") && sample.contains(".") {
            score += 2;
        }
        check(Language::Erlang, score);
    }

    #[cfg(feature = "html")]
    {
        let mut score = 0u32;
        if sample.contains("<!DOCTYPE") || sample.contains("<!doctype") {
            score += 5;
        }
        if sample.contains("<html") {
            score += 4;
        }
        if sample.contains("<div") || sample.contains("<span") {
            score += 2;
        }
        check(Language::Html, score);
    }

    #[cfg(feature = "yaml")]
    {
        let mut score = 0u32;
        // YAML is hard to detect; only strong signals
        if sample.starts_with("---\n") || sample.starts_with("---\r\n") {
            score += 4;
        }
        check(Language::Yaml, score);
    }

    #[cfg(feature = "json")]
    {
        let mut score = 0u32;
        let trimmed = sample.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            // JSON-like structure
            if !trimmed.contains("function ") && !trimmed.contains("const ") {
                score += 3;
            }
        }
        check(Language::Json, score);
    }

    #[cfg(feature = "toml")]
    {
        let mut score = 0u32;
        if sample.contains("[package]") || sample.contains("[dependencies]") {
            score += 5;
        }
        if sample.contains("[workspace]") {
            score += 5;
        }
        check(Language::Toml, score);
    }

    #[cfg(feature = "sql")]
    {
        let mut score = 0u32;
        let upper = sample.to_uppercase();
        if upper.contains("SELECT ") {
            score += 2;
        }
        if upper.contains("CREATE TABLE") {
            score += 4;
        }
        if upper.contains("INSERT INTO") {
            score += 3;
        }
        if upper.contains("ALTER TABLE") {
            score += 4;
        }
        if upper.contains("JOIN ") && upper.contains("ON ") {
            score += 3;
        }
        if upper.contains("WHERE ") {
            score += 1;
        }
        check(Language::Sql, score);
    }

    best.map(|(lang, _)| lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_rust() {
        assert_eq!(language_from_extension("rs"), Some(Language::Rust));
    }

    #[test]
    fn test_extension_python() {
        assert_eq!(language_from_extension("py"), Some(Language::Python));
    }

    #[test]
    fn test_extension_unknown() {
        assert_eq!(language_from_extension("xyz"), None);
    }

    #[test]
    fn test_shebang_python() {
        let code = "#!/usr/bin/env python3\nimport sys\n";
        assert_eq!(language_from_content(code), Some(Language::Python));
    }

    #[test]
    fn test_shebang_bash() {
        let code = "#!/bin/bash\necho hello\n";
        assert_eq!(language_from_content(code), Some(Language::Bash));
    }

    #[test]
    fn test_keywords_rust() {
        let code = r#"
use std::collections::HashMap;

pub fn process(data: &[u8]) -> Result<(), Error> {
    let mut map = HashMap::new();
    impl Foo for Bar {}
}
"#;
        assert_eq!(language_from_content(code), Some(Language::Rust));
    }

    #[test]
    fn test_keywords_go() {
        let code = r#"
package main

import "fmt"

func main() {
    x := 42
    fmt.Println(x)
}
"#;
        assert_eq!(language_from_content(code), Some(Language::Go));
    }

    #[test]
    fn test_keywords_elixir() {
        let code = r#"
defmodule MyApp.Router do
  use Phoenix.Router

  def index(conn, _params) do
    conn |> render("index.html")
  end
end
"#;
        assert_eq!(language_from_content(code), Some(Language::Elixir));
    }

    #[test]
    fn test_no_detection() {
        assert_eq!(language_from_content(""), None);
        assert_eq!(language_from_content("hello world"), None);
    }

    #[test]
    fn test_extension_sql() {
        assert_eq!(language_from_extension("sql"), Some(Language::Sql));
    }

    #[test]
    fn test_keywords_sql() {
        let code = "SELECT id, name FROM users WHERE active = true ORDER BY name;";
        assert_eq!(language_from_content(code), Some(Language::Sql));
    }
}
