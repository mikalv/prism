//! Query execution telemetry and metrics

use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{error, info, warn};

/// Metrics collected during query execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetrics {
    pub query_parse_ms: f64,
    pub ast_convert_ms: f64,
    pub tantivy_search_ms: f64,
    pub facet_compute_ms: f64,
    pub boost_apply_ms: f64,
    pub total_ms: f64,
    pub result_count: usize,
    pub total_matches: usize,
    pub facet_count: usize,
    pub collection: String,
    pub query_type: String,
    pub had_suggestions: bool,
}

impl Default for QueryMetrics {
    fn default() -> Self {
        Self {
            query_parse_ms: 0.0,
            ast_convert_ms: 0.0,
            tantivy_search_ms: 0.0,
            facet_compute_ms: 0.0,
            boost_apply_ms: 0.0,
            total_ms: 0.0,
            result_count: 0,
            total_matches: 0,
            facet_count: 0,
            collection: String::new(),
            query_type: String::new(),
            had_suggestions: false,
        }
    }
}

/// Helper for tracking query execution stages
pub struct QueryTelemetry {
    start: Instant,
    last_mark: Instant,
    stages: Vec<(String, f64)>,
}

impl QueryTelemetry {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last_mark: now,
            stages: Vec::new(),
        }
    }

    /// Mark the completion of a stage and record its duration
    pub fn mark_stage(&mut self, stage_name: &str) {
        let now = Instant::now();
        let duration_ms = (now - self.last_mark).as_secs_f64() * 1000.0;
        self.stages.push((stage_name.to_string(), duration_ms));
        self.last_mark = now;
    }

    /// Get duration of a specific stage
    pub fn stage_duration(&self, stage_name: &str) -> f64 {
        self.stages
            .iter()
            .find(|(name, _)| name == stage_name)
            .map(|(_, duration)| *duration)
            .unwrap_or(0.0)
    }

    /// Finish telemetry and build metrics
    pub fn finish(
        self,
        collection: &str,
        query_type: &str,
        result_count: usize,
        total_matches: usize,
        facet_count: usize,
    ) -> QueryMetrics {
        let total_ms = self.start.elapsed().as_secs_f64() * 1000.0;

        QueryMetrics {
            query_parse_ms: self.stage_duration("parse"),
            ast_convert_ms: self.stage_duration("ast_convert"),
            tantivy_search_ms: self.stage_duration("tantivy_search"),
            facet_compute_ms: self.stage_duration("facet_compute"),
            boost_apply_ms: self.stage_duration("boost_apply"),
            total_ms,
            result_count,
            total_matches,
            facet_count,
            collection: collection.to_string(),
            query_type: query_type.to_string(),
            had_suggestions: false,
        }
    }
}

impl Default for QueryTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

/// Log successful query execution
pub fn log_query_success(metrics: &QueryMetrics) {
    info!(
        collection = %metrics.collection,
        query_type = %metrics.query_type,
        result_count = metrics.result_count,
        total_ms = metrics.total_ms,
        tantivy_ms = metrics.tantivy_search_ms,
        boost_ms = metrics.boost_apply_ms,
        "Query executed successfully"
    );

    // Warn on slow queries
    if metrics.total_ms > 500.0 {
        warn!(
            collection = %metrics.collection,
            total_ms = metrics.total_ms,
            tantivy_ms = metrics.tantivy_search_ms,
            "Slow query detected"
        );
    }
}

/// Log query execution failure
pub fn log_query_error(collection: &str, error: &str, query: &str) {
    error!(
        collection = %collection,
        error = %error,
        query = %query,
        "Query execution failed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_telemetry_stage_tracking() {
        let mut telemetry = QueryTelemetry::new();

        sleep(Duration::from_millis(10));
        telemetry.mark_stage("parse");

        sleep(Duration::from_millis(10));
        telemetry.mark_stage("ast_convert");

        let parse_ms = telemetry.stage_duration("parse");
        let convert_ms = telemetry.stage_duration("ast_convert");

        assert!(parse_ms >= 9.0, "Parse should be ~10ms, got {}", parse_ms);
        assert!(
            convert_ms >= 9.0,
            "Convert should be ~10ms, got {}",
            convert_ms
        );
    }

    #[test]
    fn test_telemetry_finish() {
        let mut telemetry = QueryTelemetry::new();
        telemetry.mark_stage("parse");
        telemetry.mark_stage("tantivy_search");

        let metrics = telemetry.finish("test_collection", "term", 10, 100, 2);

        assert_eq!(metrics.collection, "test_collection");
        assert_eq!(metrics.query_type, "term");
        assert_eq!(metrics.result_count, 10);
        assert_eq!(metrics.total_matches, 100);
        assert_eq!(metrics.facet_count, 2);
    }

    #[test]
    fn test_query_metrics_default() {
        let metrics = QueryMetrics::default();
        assert_eq!(metrics.result_count, 0);
        assert_eq!(metrics.total_ms, 0.0);
    }
}
