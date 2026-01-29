use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue};
use std::default::Default;
use tantivy::fastfield::Column;
use tantivy::{DocId, Result as TantivyResult, Score, Searcher};

pub struct SumAgg {
    field: String,
}

pub struct SumPrepared {
    field: String,
}

#[allow(dead_code)]
pub struct SumSegment {
    field: String,
    sum: f64,
    fast_field_reader: Option<Column<u64>>,
}

#[derive(Debug, Clone)]
pub struct SumFruit(f64);

impl Default for SumFruit {
    fn default() -> Self {
        SumFruit(0.0)
    }
}

impl Default for SumPrepared {
    fn default() -> Self {
        SumPrepared {
            field: String::new(),
        }
    }
}

impl SumAgg {
    pub fn sum(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl Agg for SumAgg {
    type Fruit = SumFruit;
    type Child = SumPrepared;

    fn prepare(&self, _: &Searcher) -> TantivyResult<Self::Child> {
        Ok(SumPrepared {
            field: self.field.clone(),
        })
    }
}

impl PreparedAgg for SumPrepared {
    type Fruit = SumFruit;
    type Child = SumSegment;

    fn create_fruit(&self) -> Self::Fruit {
        SumFruit(0.0)
    }

    fn for_segment(&self, ctx: &AggSegmentContext) -> TantivyResult<Self::Child> {
        let fast_field_reader = ctx.reader.fast_fields().u64(&self.field).ok();

        Ok(SumSegment {
            field: self.field.clone(),
            sum: 0.0,
            fast_field_reader,
        })
    }

    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit) {
        acc.0 += fruit.0;
    }
}

impl SegmentAgg for SumSegment {
    type Fruit = SumFruit;

    fn create_fruit(&self) -> Self::Fruit {
        SumFruit(self.sum)
    }

    fn collect(&mut self, doc: DocId, _: Score, fruit: &mut Self::Fruit) {
        if let Some(ref reader) = self.fast_field_reader {
            if let Some(v) = reader.first(doc) {
                let v_f64 = v as f64;
                self.sum += v_f64;
                fruit.0 = self.sum;
            }
        }
    }
}

impl SumAgg {
    pub fn into_result(name: String, fruit: SumFruit) -> AggregationResult {
        AggregationResult {
            name,
            value: AggregationValue::Single(fruit.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sum_aggregation() {
        let fruit = SumFruit(150.5);
        let result = SumAgg::into_result("total_revenue".to_string(), fruit);

        assert_eq!(result.name, "total_revenue");
        match result.value {
            AggregationValue::Single(val) => assert_eq!(val, 150.5),
            _ => panic!("Expected Single value"),
        }
    }

    #[test]
    fn test_sum_default() {
        let fruit = SumFruit::default();
        assert_eq!(fruit.0, 0.0);
    }

    #[test]
    fn test_sum_agg_builder() {
        let agg = SumAgg::sum("price");
        assert_eq!(agg.field, "price");
    }
}
