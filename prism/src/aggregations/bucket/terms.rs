use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue, Bucket};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::default::Default;
use tantivy::{DocId, Result as TantivyResult, Score, Searcher};

pub struct TermsAgg {
    field: String,
    size: usize,
}

pub struct TermsPrepared {
    field: String,
    size: usize,
}

#[allow(dead_code)]
pub struct TermsSegment {
    field: String,
    size: usize,
    counts: HashMap<String, u64>,
}

#[derive(Debug, Clone)]
pub struct TermsFruit(HashMap<String, u64>);

impl Default for TermsFruit {
    fn default() -> Self {
        TermsFruit(HashMap::new())
    }
}

impl Default for TermsPrepared {
    fn default() -> Self {
        TermsPrepared {
            field: String::new(),
            size: 10,
        }
    }
}

impl TermsAgg {
    pub fn new(field: impl Into<String>, size: usize) -> Self {
        Self {
            field: field.into(),
            size,
        }
    }
}

impl Agg for TermsAgg {
    type Fruit = TermsFruit;
    type Child = TermsPrepared;

    fn prepare(&self, _: &Searcher) -> TantivyResult<Self::Child> {
        Ok(TermsPrepared {
            field: self.field.clone(),
            size: self.size,
        })
    }
}

impl PreparedAgg for TermsPrepared {
    type Fruit = TermsFruit;
    type Child = TermsSegment;

    fn create_fruit(&self) -> Self::Fruit {
        TermsFruit(HashMap::new())
    }

    fn for_segment(&self, _ctx: &AggSegmentContext) -> TantivyResult<Self::Child> {
        Ok(TermsSegment {
            field: self.field.clone(),
            size: self.size,
            counts: HashMap::new(),
        })
    }

    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit) {
        for (term, count) in fruit.0 {
            *acc.0.entry(term.clone()).or_insert(0) += count;
        }
    }
}

impl SegmentAgg for TermsSegment {
    type Fruit = TermsFruit;

    fn create_fruit(&self) -> Self::Fruit {
        TermsFruit(self.counts.clone())
    }

    fn collect(&mut self, _doc: DocId, _: Score, _fruit: &mut Self::Fruit) {
        // TODO: Implement proper string field collection in Tantivy 0.22
        // For now, we'll skip aggregation collection until API is resolved
    }
}

impl TermsAgg {
    pub fn into_result(&self, name: String, fruit: TermsFruit) -> AggregationResult {
        let mut heap = BinaryHeap::new();
        for (term, count) in fruit.0 {
            heap.push(Reverse((count, term)));
        }

        let mut buckets = Vec::new();
        let max_buckets = heap.len().min(self.size);
        for _ in 0..max_buckets {
            if let Some(Reverse((count, term))) = heap.pop() {
                buckets.push(Bucket {
                    key: term,
                    doc_count: count,
                    from: None,
                    to: None,
                    sub_aggs: None,
                });
            }
        }

        buckets.sort_by(|a, b| b.doc_count.cmp(&a.doc_count));

        AggregationResult {
            name,
            value: AggregationValue::Buckets(buckets),
        }
    }
}
