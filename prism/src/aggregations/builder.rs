use crate::aggregations::types::AggregationType;

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
        AggregationType::Histogram { field, interval }
    }

    pub fn date_histogram(field: String, calendar_interval: String) -> AggregationType {
        AggregationType::DateHistogram {
            field,
            calendar_interval,
        }
    }
}
