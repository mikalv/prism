use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue};
use std::default::Default;
use tantivy::fastfield::Column;
use tantivy::{DocId, Result as TantivyResult, Score, Searcher};

pub struct MinMaxAgg {
    field: String,
    is_min: bool,
}

pub struct MinMaxPrepared {
    field: String,
    is_min: bool,
}

#[allow(dead_code)]
pub struct MinMaxSegment {
    field: String,
    is_min: bool,
    value: Option<f64>,
    fast_field_reader: Option<Column<u64>>,
}

#[derive(Debug, Clone, Default)]
pub struct MinMaxFruit(Option<f64>);

impl Default for MinMaxPrepared {
    fn default() -> Self {
        MinMaxPrepared {
            field: String::new(),
            is_min: true,
        }
    }
}

impl MinMaxAgg {
    pub fn min(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            is_min: true,
        }
    }

    pub fn max(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            is_min: false,
        }
    }
}

impl Agg for MinMaxAgg {
    type Fruit = MinMaxFruit;
    type Child = MinMaxPrepared;

    fn prepare(&self, _: &Searcher) -> TantivyResult<Self::Child> {
        Ok(MinMaxPrepared {
            field: self.field.clone(),
            is_min: self.is_min,
        })
    }
}

impl PreparedAgg for MinMaxPrepared {
    type Fruit = MinMaxFruit;
    type Child = MinMaxSegment;

    fn create_fruit(&self) -> Self::Fruit {
        MinMaxFruit(None)
    }

    fn for_segment(&self, ctx: &AggSegmentContext) -> TantivyResult<Self::Child> {
        let fast_field_reader = ctx.reader.fast_fields().u64(&self.field).ok();

        Ok(MinMaxSegment {
            field: self.field.clone(),
            is_min: self.is_min,
            value: None,
            fast_field_reader,
        })
    }

    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit) {
        match (acc.0, fruit.0) {
            (None, Some(v)) => acc.0 = Some(v),
            (Some(a), Some(v)) if self.is_min => acc.0 = Some(a.min(v)),
            (Some(a), Some(v)) => acc.0 = Some(a.max(v)),
            _ => {}
        }
    }
}

impl SegmentAgg for MinMaxSegment {
    type Fruit = MinMaxFruit;

    fn create_fruit(&self) -> Self::Fruit {
        MinMaxFruit(self.value)
    }

    fn collect(&mut self, doc: DocId, _: Score, fruit: &mut Self::Fruit) {
        if let Some(ref reader) = self.fast_field_reader {
            if let Some(v) = reader.first(doc) {
                let v_f64 = v as f64;
                fruit.0 = Some(self.value.map_or(v_f64, |acc| {
                    if self.is_min {
                        acc.min(v_f64)
                    } else {
                        acc.max(v_f64)
                    }
                }));
            }
        }
    }
}

impl MinMaxAgg {
    pub fn into_result(name: String, fruit: MinMaxFruit) -> AggregationResult {
        AggregationResult {
            name,
            value: AggregationValue::Single(fruit.0.unwrap_or(0.0)),
        }
    }
}
