use super::{AggregationResult, Bucket};
use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

/// Interval for date histogram aggregation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateInterval {
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl DateInterval {
    pub fn parse_interval(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "hour" => Some(DateInterval::Hour),
            "day" => Some(DateInterval::Day),
            "week" => Some(DateInterval::Week),
            "month" => Some(DateInterval::Month),
            "year" => Some(DateInterval::Year),
            _ => None,
        }
    }

    /// Round timestamp down to interval boundary
    pub fn floor(&self, dt: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            DateInterval::Hour => dt
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap(),
            DateInterval::Day => dt
                .with_hour(0)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap(),
            DateInterval::Week => {
                let days_from_monday = dt.weekday().num_days_from_monday();
                let start_of_week = dt - Duration::days(days_from_monday as i64);
                start_of_week
                    .with_hour(0)
                    .unwrap()
                    .with_minute(0)
                    .unwrap()
                    .with_second(0)
                    .unwrap()
                    .with_nanosecond(0)
                    .unwrap()
            }
            DateInterval::Month => dt
                .with_day(1)
                .unwrap()
                .with_hour(0)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap(),
            DateInterval::Year => dt
                .with_month(1)
                .unwrap()
                .with_day(1)
                .unwrap()
                .with_hour(0)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap(),
        }
    }
}

/// Aggregate timestamps into histogram buckets
pub fn aggregate_date_histogram(
    timestamps: Vec<DateTime<Utc>>,
    interval: DateInterval,
) -> AggregationResult {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for ts in timestamps {
        let bucket_key = interval.floor(ts).to_rfc3339();
        *counts.entry(bucket_key).or_insert(0) += 1;
    }

    let mut buckets: Vec<_> = counts
        .into_iter()
        .map(|(key, count)| Bucket { key, count })
        .collect();

    // Sort by key (chronological order)
    buckets.sort_by(|a, b| a.key.cmp(&b.key));

    AggregationResult {
        field: "".to_string(),
        buckets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_interval_from_str() {
        assert_eq!(
            DateInterval::parse_interval("hour"),
            Some(DateInterval::Hour)
        );
        assert_eq!(DateInterval::parse_interval("Day"), Some(DateInterval::Day));
        assert_eq!(
            DateInterval::parse_interval("WEEK"),
            Some(DateInterval::Week)
        );
        assert_eq!(DateInterval::parse_interval("invalid"), None);
    }

    #[test]
    fn test_floor_hour() {
        let dt = Utc::now()
            .with_hour(14)
            .unwrap()
            .with_minute(35)
            .unwrap()
            .with_second(42)
            .unwrap();

        let floored = DateInterval::Hour.floor(dt);
        assert_eq!(floored.hour(), 14);
        assert_eq!(floored.minute(), 0);
        assert_eq!(floored.second(), 0);
    }

    #[test]
    fn test_floor_day() {
        let dt = Utc::now().with_hour(14).unwrap().with_minute(35).unwrap();

        let floored = DateInterval::Day.floor(dt);
        assert_eq!(floored.hour(), 0);
        assert_eq!(floored.minute(), 0);
    }

    #[test]
    fn test_aggregate_date_histogram_hours() {
        let base = Utc::now()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let timestamps = vec![
            base + Duration::minutes(10),
            base + Duration::minutes(20),
            base + Duration::hours(1) + Duration::minutes(5),
            base + Duration::hours(1) + Duration::minutes(15),
            base + Duration::hours(1) + Duration::minutes(25),
        ];

        let result = aggregate_date_histogram(timestamps, DateInterval::Hour);

        assert_eq!(result.buckets.len(), 2);
        assert_eq!(result.buckets[0].count, 2); // First hour
        assert_eq!(result.buckets[1].count, 3); // Second hour
    }

    #[test]
    fn test_aggregate_date_histogram_days() {
        let base = Utc::now()
            .with_hour(0)
            .unwrap()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let timestamps = vec![
            base + Duration::hours(2),
            base + Duration::hours(10),
            base + Duration::days(1) + Duration::hours(3),
            base + Duration::days(2) + Duration::hours(5),
        ];

        let result = aggregate_date_histogram(timestamps, DateInterval::Day);

        assert_eq!(result.buckets.len(), 3);
        assert_eq!(result.buckets[0].count, 2); // Day 0
        assert_eq!(result.buckets[1].count, 1); // Day 1
        assert_eq!(result.buckets[2].count, 1); // Day 2
    }

    #[test]
    fn test_buckets_sorted_chronologically() {
        let base = Utc::now();
        let timestamps = vec![base + Duration::days(2), base, base + Duration::days(1)];

        let result = aggregate_date_histogram(timestamps, DateInterval::Day);

        // Should be sorted by date (oldest to newest)
        assert!(result.buckets[0].key < result.buckets[1].key);
        assert!(result.buckets[1].key < result.buckets[2].key);
    }
}
