use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue, Bucket};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use tantivy::fastfield::StrColumn;
use tantivy::{DocId, Result as TantivyResult, Score, Searcher};

pub struct TermsAgg {
    field: String,
    size: usize,
}

pub struct TermsPrepared {
    field: String,
    size: usize,
}

pub struct TermsSegment {
    field: String,
    size: usize,
    counts: HashMap<String, u64>,
    fast_field_reader: Option<StrColumn>,
}

pub struct TermsFruit(HashMap<String, u64>);

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

    fn for_segment(&self, ctx: &AggSegmentContext) -> TantivyResult<Self::Child> {
        let fast_field_reader = ctx.reader.fast_fields().str(&self.field).ok().flatten();

        Ok(TermsSegment {
            fast_field_reader,
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

    fn collect(&mut self, doc: DocId, _: Score, fruit: &mut Self::Fruit) {
        if let Some(ref reader) = self.fast_field_reader {
            if let Some(term) = reader.first(doc) {
                let term_str: &str = &term;
                *fruit.0.entry(term_str.to_string()).or_insert(0) += 1;
            }
        }
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
