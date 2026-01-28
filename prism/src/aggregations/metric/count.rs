use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue};
use tantivy::{DocId, Result as TantivyResult, Score, Searcher};

pub struct CountAgg;
pub struct CountPrepared;
pub struct CountSegment;
pub struct CountFruit(u64);

impl Agg for CountAgg {
    type Fruit = CountFruit;
    type Child = CountPrepared;

    fn prepare(&self, _: &Searcher) -> TantivyResult<Self::Child> {
        Ok(CountPrepared)
    }
}

impl PreparedAgg for CountPrepared {
    type Fruit = CountFruit;
    type Child = CountSegment;

    fn create_fruit(&self) -> Self::Fruit {
        CountFruit(0)
    }

    fn for_segment(&self, _: &AggSegmentContext) -> TantivyResult<Self::Child> {
        Ok(CountSegment)
    }

    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit) {
        acc.0 += fruit.0;
    }
}

impl SegmentAgg for CountSegment {
    type Fruit = CountFruit;

    fn create_fruit(&self) -> Self::Fruit {
        CountFruit(0)
    }

    fn collect(&mut self, _: DocId, _: Score, fruit: &mut Self::Fruit) {
        fruit.0 += 1;
    }
}

impl CountAgg {
    pub fn into_result(name: String, fruit: CountFruit) -> AggregationResult {
        AggregationResult {
            name,
            value: AggregationValue::Single(fruit.0 as f64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_aggregation() {
        let fruit = CountFruit(42);
        let result = CountAgg::into_result("total_docs".to_string(), fruit);

        assert_eq!(result.name, "total_docs");
        match result.value {
            AggregationValue::Single(val) => assert_eq!(val, 42.0),
            _ => panic!("Expected Single value"),
        }
    }
}
