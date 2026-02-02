use crate::aggregations::types::{AggregationType, HistogramBounds, RangeEntry};
use std::collections::HashMap;

impl AggregationType {
    pub fn count() -> AggregationType {
        AggregationType::Count
    }

    pub fn min(field: String) -> AggregationType {
        AggregationType::Min { field }
    }

    pub fn max(field: String) -> AggregationType {
        AggregationType::Max { field }
    }

    pub fn sum(field: String) -> AggregationType {
        AggregationType::Sum { field }
    }

    pub fn avg(field: String) -> AggregationType {
        AggregationType::Avg { field }
    }

    pub fn terms(field: String) -> AggregationType {
        AggregationType::Terms {
            field,
            size: Some(10),
        }
    }

    pub fn terms_with_size(field: String, size: usize) -> AggregationType {
        AggregationType::Terms {
            field,
            size: Some(size),
        }
    }

    pub fn histogram(field: String, interval: f64) -> AggregationType {
        AggregationType::Histogram {
            field,
            interval,
            min_doc_count: None,
            extended_bounds: None,
        }
    }

    pub fn histogram_with_bounds(field: String, interval: f64, min: f64, max: f64) -> AggregationType {
        AggregationType::Histogram {
            field,
            interval,
            min_doc_count: None,
            extended_bounds: Some(HistogramBounds { min, max }),
        }
    }

    pub fn date_histogram(field: String, calendar_interval: String) -> AggregationType {
        AggregationType::DateHistogram {
            field,
            calendar_interval,
            min_doc_count: None,
        }
    }

    pub fn percentiles(field: String, percents: Vec<f64>) -> AggregationType {
        AggregationType::Percentiles { field, percents }
    }

    pub fn range(field: String, ranges: Vec<RangeEntry>) -> AggregationType {
        AggregationType::Range { field, ranges }
    }

    pub fn filter(filter: String) -> AggregationType {
        AggregationType::Filter { filter }
    }

    pub fn filters(filters: HashMap<String, String>) -> AggregationType {
        AggregationType::Filters { filters }
    }

    pub fn global() -> AggregationType {
        AggregationType::Global {}
    }
}
