use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue};
use std::default::Default;
use tantivy::fastfield::Column;
use tantivy::{DocId, Result as TantivyResult, Score, Searcher};

pub struct AvgAgg {
    field: String,
}

pub struct AvgPrepared {
    field: String,
}

#[allow(dead_code)]
pub struct AvgSegment {
    field: String,
    sum: f64,
    count: u64,
    fast_field_reader: Option<Column<u64>>,
}

#[derive(Debug, Clone)]
pub struct AvgFruit {
    sum: f64,
    count: u64,
}

impl Default for AvgFruit {
    fn default() -> Self {
        AvgFruit { sum: 0.0, count: 0 }
    }
}

impl Default for AvgPrepared {
    fn default() -> Self {
        AvgPrepared {
            field: String::new(),
        }
    }
}

impl AvgAgg {
    pub fn avg(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl Agg for AvgAgg {
    type Fruit = AvgFruit;
    type Child = AvgPrepared;

    fn prepare(&self, _: &Searcher) -> TantivyResult<Self::Child> {
        Ok(AvgPrepared {
            field: self.field.clone(),
        })
    }
}

impl PreparedAgg for AvgPrepared {
    type Fruit = AvgFruit;
    type Child = AvgSegment;

    fn create_fruit(&self) -> Self::Fruit {
        AvgFruit { sum: 0.0, count: 0 }
    }

    fn for_segment(&self, ctx: &AggSegmentContext) -> TantivyResult<Self::Child> {
        let fast_field_reader = ctx.reader.fast_fields().u64(&self.field).ok();

        Ok(AvgSegment {
            field: self.field.clone(),
            sum: 0.0,
            count: 0,
            fast_field_reader,
        })
    }

    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit) {
        acc.sum += fruit.sum;
        acc.count += fruit.count;
    }
}

impl SegmentAgg for AvgSegment {
    type Fruit = AvgFruit;

    fn create_fruit(&self) -> Self::Fruit {
        AvgFruit {
            sum: self.sum,
            count: self.count,
        }
    }

    fn collect(&mut self, doc: DocId, _: Score, fruit: &mut Self::Fruit) {
        if let Some(ref reader) = self.fast_field_reader {
            if let Some(v) = reader.first(doc) {
                let v_f64 = v as f64;
                self.sum += v_f64;
                self.count += 1;
                fruit.sum = self.sum;
                fruit.count = self.count;
            }
        }
    }
}

impl AvgAgg {
    pub fn into_result(name: String, fruit: AvgFruit) -> AggregationResult {
        let avg = if fruit.count > 0 {
            fruit.sum / fruit.count as f64
        } else {
            0.0
        };

        AggregationResult {
            name,
            value: AggregationValue::Single(avg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_avg_aggregation() {
        let fruit = AvgFruit {
            sum: 150.0,
            count: 3,
        };
        let result = AvgAgg::into_result("avg_price".to_string(), fruit);

        assert_eq!(result.name, "avg_price");
        match result.value {
            AggregationValue::Single(val) => assert_eq!(val, 50.0),
            _ => panic!("Expected Single value"),
        }
    }

    #[test]
    fn test_avg_zero_count() {
        let fruit = AvgFruit { sum: 0.0, count: 0 };
        let result = AvgAgg::into_result("avg_price".to_string(), fruit);

        match result.value {
            AggregationValue::Single(val) => assert_eq!(val, 0.0),
            _ => panic!("Expected Single value"),
        }
    }

    #[test]
    fn test_avg_default() {
        let fruit = AvgFruit::default();
        assert_eq!(fruit.sum, 0.0);
        assert_eq!(fruit.count, 0);
    }

    #[test]
    fn test_avg_agg_builder() {
        let agg = AvgAgg::avg("rating");
        assert_eq!(agg.field, "rating");
    }
}
