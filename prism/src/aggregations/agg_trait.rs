use tantivy::query::Scorer;
use tantivy::{DocId, Result as TantivyResult, Score, Searcher, SegmentReader};

pub type SegmentLocalId = u32;

/// Context for segment aggregation
pub struct AggSegmentContext<'r, 's> {
    pub segment_ord: SegmentLocalId,
    pub reader: &'r SegmentReader,
    pub scorer: &'s dyn Scorer,
}

/// High-level aggregation API
pub trait Agg: Send + Sync {
    type Fruit: Send + 'static + Default;
    type Child: PreparedAgg + Send + 'static;

    fn prepare(&self, searcher: &Searcher) -> TantivyResult<Self::Child>;
}

/// Prepared aggregation for segment collection
pub trait PreparedAgg: Send + Sync {
    type Fruit: Send + 'static + Default;
    type Child: SegmentAgg + Send + 'static;

    fn create_fruit(&self) -> Self::Fruit;
    fn for_segment(&self, ctx: &AggSegmentContext) -> TantivyResult<Self::Child>;
    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit);
}

/// Segment-level aggregation
pub trait SegmentAgg: Send + Sync {
    type Fruit: Send + 'static + Default;

    fn create_fruit(&self) -> Self::Fruit;
    fn collect(&mut self, doc: DocId, score: Score, fruit: &mut Self::Fruit);
}
