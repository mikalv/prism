//! Template pattern matching
//!
//! Matches index names against template patterns (with wildcard support).

use super::types::{IndexTemplate, TemplateMatch, TemplateRegistry};
use regex::Regex;

/// Matches index names against template patterns
pub struct TemplateMatcher;

impl TemplateMatcher {
    /// Convert a glob pattern to a regex
    /// Supports: * (any characters), ? (single character)
    fn pattern_to_regex(pattern: &str) -> Result<Regex, regex::Error> {
        let mut regex_str = String::from("^");

        for c in pattern.chars() {
            match c {
                '*' => regex_str.push_str(".*"),
                '?' => regex_str.push('.'),
                '.' | '+' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                    regex_str.push('\\');
                    regex_str.push(c);
                }
                _ => regex_str.push(c),
            }
        }

        regex_str.push('$');
        Regex::new(&regex_str)
    }

    /// Check if an index name matches a pattern
    pub fn matches_pattern(index_name: &str, pattern: &str) -> bool {
        match Self::pattern_to_regex(pattern) {
            Ok(regex) => regex.is_match(index_name),
            Err(_) => false,
        }
    }

    /// Find all matching templates for an index name, sorted by priority (highest first)
    pub fn find_matches(registry: &TemplateRegistry, index_name: &str) -> Vec<TemplateMatch> {
        let mut matches: Vec<TemplateMatch> = registry
            .templates
            .values()
            .filter_map(|template| {
                // Check each pattern
                for pattern in &template.index_patterns {
                    if Self::matches_pattern(index_name, pattern) {
                        return Some(TemplateMatch {
                            template: template.clone(),
                            matched_pattern: pattern.clone(),
                        });
                    }
                }
                None
            })
            .collect();

        // Sort by priority (highest first)
        matches.sort_by(|a, b| b.template.priority.cmp(&a.template.priority));

        matches
    }

    /// Find the best matching template (highest priority)
    pub fn find_best_match(registry: &TemplateRegistry, index_name: &str) -> Option<TemplateMatch> {
        Self::find_matches(registry, index_name).into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::types::IndexTemplate;
    use std::collections::HashMap;

    fn create_test_template(name: &str, patterns: Vec<&str>, priority: u32) -> IndexTemplate {
        IndexTemplate {
            name: name.to_string(),
            index_patterns: patterns.into_iter().map(String::from).collect(),
            priority,
            settings: Default::default(),
            schema: Default::default(),
            aliases: HashMap::new(),
        }
    }

    #[test]
    fn test_exact_match() {
        assert!(TemplateMatcher::matches_pattern("logs", "logs"));
        assert!(!TemplateMatcher::matches_pattern("logs", "log"));
    }

    #[test]
    fn test_wildcard_suffix() {
        assert!(TemplateMatcher::matches_pattern(
            "logs-2026.01.29",
            "logs-*"
        ));
        assert!(TemplateMatcher::matches_pattern("logs-", "logs-*"));
        assert!(!TemplateMatcher::matches_pattern("log-2026", "logs-*"));
    }

    #[test]
    fn test_wildcard_prefix() {
        assert!(TemplateMatcher::matches_pattern("my-logs", "*-logs"));
        assert!(TemplateMatcher::matches_pattern("-logs", "*-logs"));
    }

    #[test]
    fn test_wildcard_middle() {
        assert!(TemplateMatcher::matches_pattern(
            "logs-2026-data",
            "logs-*-data"
        ));
        assert!(TemplateMatcher::matches_pattern(
            "logs--data",
            "logs-*-data"
        ));
    }

    #[test]
    fn test_question_mark() {
        assert!(TemplateMatcher::matches_pattern("log1", "log?"));
        assert!(TemplateMatcher::matches_pattern("logA", "log?"));
        assert!(!TemplateMatcher::matches_pattern("log", "log?"));
        assert!(!TemplateMatcher::matches_pattern("log12", "log?"));
    }

    #[test]
    fn test_multiple_wildcards() {
        assert!(TemplateMatcher::matches_pattern(
            "my-logs-2026-prod",
            "*-logs-*-prod"
        ));
    }

    #[test]
    fn test_find_matches_priority() {
        let mut registry = TemplateRegistry::new();

        registry.upsert(create_test_template("low", vec!["logs-*"], 10));
        registry.upsert(create_test_template("high", vec!["logs-*"], 100));
        registry.upsert(create_test_template("medium", vec!["logs-*"], 50));

        let matches = TemplateMatcher::find_matches(&registry, "logs-2026");

        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].template.name, "high");
        assert_eq!(matches[1].template.name, "medium");
        assert_eq!(matches[2].template.name, "low");
    }

    #[test]
    fn test_find_best_match() {
        let mut registry = TemplateRegistry::new();

        registry.upsert(create_test_template("generic", vec!["*"], 1));
        registry.upsert(create_test_template("logs", vec!["logs-*"], 100));

        let best = TemplateMatcher::find_best_match(&registry, "logs-2026");
        assert!(best.is_some());
        assert_eq!(best.unwrap().template.name, "logs");

        let best = TemplateMatcher::find_best_match(&registry, "metrics-2026");
        assert!(best.is_some());
        assert_eq!(best.unwrap().template.name, "generic");
    }

    #[test]
    fn test_no_match() {
        let mut registry = TemplateRegistry::new();
        registry.upsert(create_test_template("logs", vec!["logs-*"], 100));

        let matches = TemplateMatcher::find_matches(&registry, "metrics-2026");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_special_characters() {
        assert!(TemplateMatcher::matches_pattern("logs.2026", "logs.2026"));
        assert!(TemplateMatcher::matches_pattern("logs.2026", "logs.*"));
        assert!(TemplateMatcher::matches_pattern("logs[1]", "logs[1]"));
    }
}
