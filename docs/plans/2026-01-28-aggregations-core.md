# Aggregations Core Framework Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable analytics queries like "top 10 categories by count" or "average price per brand" via aggregations framework.

**Architecture:** Three-tier trait system from inspiration/tantivy-aggregations:
1. `Agg` - High-level API, prepares aggregation
2. `PreparedAgg` - Creates segment collectors, merges results
3. `SegmentAgg` - Collects data from individual documents

**Tech Stack:** Tantivy `Collector` trait, `tantivy::Searcher`, fast field readers, serde for JSON serialization

---

## Task 1: Create Core Aggregation Module

**Files:**
- Create: `prism/src/aggregations/mod.rs`
- Create: `prism/src/aggregations/types.rs`
- Modify: `prism/src/lib.rs`

**Step 1.1: Create mod.rs**
```rust
mod types;
mod agg_trait;
mod bucket;
mod metric;

pub use types::*;
pub use agg_trait::{Agg, PreparedAgg, SegmentAgg, AggSegmentContext};
```

**Step 1.2: Create types.rs**
```rust
use serde::{Deserialize, Serialize};
use tantivy::Schema;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationRequest {
    pub name: String,
    #[serde(flatten)]
    pub agg_type: AggregationType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AggregationType {
    Count,
    Min { field: String },
    Max { field: String },
    Sum { field: String },
    Avg { field: String },
    Stats { field: String },
    Terms { field: String, size: Option<usize> },
    Histogram { field: String, interval: f64 },
    DateHistogram { field: String, calendar_interval: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResult {
    pub name: String,
    #[serde(flatten)]
    pub value: AggregationValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AggregationValue {
    Single(f64),
    Stats(StatsResult),
    Buckets(Vec<Bucket>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResult {
    pub count: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sum: Option<f64>,
    pub avg: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub key: String,
    pub doc_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_aggs: Option<Vec<AggregationResult>>,
}
```

**Step 1.3: Add to lib.rs**
```rust
pub mod aggregations;
```

**Step 1.4: Verify compilation**
Run: `cargo check -p prism`
Expected: Compiles without errors

**Step 1.5: Commit**
```bash
git add prism/src/aggregations prism/src/lib.rs
git commit -m "feat(aggregations): add core types module"
```

---

## Task 2: Implement Three-Tier Trait System

**Files:**
- Create: `prism/src/aggregations/agg_trait.rs`

**Step 2.1: Implement traits**
```rust
use std::any::Any;
use tantivy::{DocId, Result as TantivyResult, Score, SegmentLocalId, SegmentReader, Searcher, Scorer};

/// Context for segment aggregation
pub struct AggSegmentContext<'r> {
    pub segment_ord: SegmentLocalId,
    pub reader: &'r SegmentReader,
    pub scorer: &'r dyn Scorer,
}

/// High-level aggregation API
pub trait Agg: Send + Sync {
    type Fruit: Send + 'static;
    type Child: PreparedAgg<Fruit = Self::Fruit> + Send + 'static;

    fn prepare(&self, searcher: &Searcher) -> TantivyResult<Self::Child>;
}

/// Prepared aggregation for segment collection
pub trait PreparedAgg: Send + Sync {
    type Fruit: Send + 'static;
    type Child: SegmentAgg<Fruit = Self::Fruit> + Send + 'static;

    fn create_fruit(&self) -> Self::Fruit;
    fn for_segment(&self, ctx: &AggSegmentContext) -> TantivyResult<Self::Child>;
    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit);
}

/// Segment-level aggregation
pub trait SegmentAgg: Send + Sync {
    type Fruit: Send + 'static;

    fn create_fruit(&self) -> Self::Fruit;
    fn collect(&mut self, doc: DocId, score: Score, fruit: &mut Self::Fruit);
}

/// Type-erased fruit for dynamic dispatch
pub type BoxedFruit = Box<dyn Any + Send + Sync>;

/// Type-erased segment agg for dynamic dispatch
pub type BoxedSegmentAgg = Box<dyn Any + Send + Sync>;
```

**Step 2.2: Verify compilation**
Run: `cargo check -p prism`
Expected: Compiles without errors

**Step 2.3: Commit**
```bash
git add prism/src/aggregations/agg_trait.rs
git commit -m "feat(aggregations): implement three-tier trait system"
```

---

## Task 3: Implement Count Aggregation

**Files:**
- Create: `prism/src/aggregations/metric/count.rs`
- Create: `prism/src/aggregations/metric/mod.rs`

**Step 3.1: Create count aggregation**
```rust
use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue};
use tantivy::{Result as TantivyResult, Searcher};
use std::sync::atomic::{AtomicU64, Ordering};

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
```

**Step 3.2: Create mod.rs for metrics**
```rust
mod count;

pub use count::CountAgg;
```

**Step 3.3: Verify compilation**
Run: `cargo check -p prism`
Expected: Compiles without errors

**Step 3.4: Commit**
```bash
git add prism/src/aggregations/metric
git commit -m "feat(aggregations): implement count aggregation"
```

---

## Task 4: Implement Min/Max Aggregation

**Files:**
- Create: `prism/src/aggregations/metric/minmax.rs`

**Step 4.1: Implement min/max**
```rust
use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue};
use tantivy::{Result as TantivyResult, Searcher, fastfield::FastFieldReader};
use std::cmp::{Ordering, PartialOrd};

pub struct MinMaxAgg {
    field: String,
    is_min: bool,
}

pub struct MinMaxPrepared {
    field: String,
    is_min: bool,
}

pub struct MinMaxSegment<T> {
    reader: T,
    field: String,
    is_min: bool,
    value: Option<f64>,
}

pub struct MinMaxFruit(Option<f64>);

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

impl<T: FastFieldReader<f64>> Agg for MinMaxAgg {
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
    type Child = MinMaxSegment<Tantivy::fastfield::FastValueReader<f64>>;

    fn create_fruit(&self) -> Self::Fruit {
        MinMaxFruit(None)
    }

    fn for_segment(&self, ctx: &AggSegmentContext) -> TantivyResult<Self::Child> {
        let field = ctx
            .reader
            .fast_fields()
            .u64(&self.field)
            .ok_or_else(|| tantivy::TantivyError::SchemaError(format!(
                "field {} not found or not u64/f64",
                self.field
            )))?;

        Ok(MinMaxSegment {
            reader: field,
            field: self.field.clone(),
            is_min: self.is_min,
            value: None,
        })
    }

    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit) {
        match (&acc.0, &fruit.0) {
            (None, Some(v)) => acc.0 = Some(v),
            (Some(a), Some(v)) if self.is_min => acc.0 = Some(a.min(v)),
            (Some(a), Some(v)) => acc.0 = Some(a.max(v)),
            _ => {}
        }
    }
}

impl<T: FastFieldReader<f64>> SegmentAgg for MinMaxSegment<T> {
    type Fruit = MinMaxFruit;

    fn create_fruit(&self) -> Self::Fruit {
        MinMaxFruit(self.value)
    }

    fn collect(&mut self, doc: DocId, _: Score, fruit: &mut Self::Fruit) {
        if let Some(v) = self.reader.get(doc) {
            fruit.0 = Some(self.value.map_or(v, |acc| {
                if self.is_min {
                    acc.min(v)
                } else {
                    acc.max(v)
                }
            }));
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
```

**Step 4.2: Update mod.rs**
```rust
mod count;
mod minmax;

pub use count::CountAgg;
pub use minmax::{MinMaxAgg, MinMaxAgg, MaxAgg};
```

**Step 4.3: Verify compilation**
Run: `cargo check -p prism`
Expected: Compiles without errors

**Step 4.4: Commit**
```bash
git add prism/src/aggregations/metric
git commit -m "feat(aggregations): implement min/max aggregations"
```

---

## Task 5: Implement Terms Bucket Aggregation

**Files:**
- Create: `prism/src/aggregations/bucket/terms.rs`
- Create: `prism/src/aggregations/bucket/mod.rs`

**Step 5.1: Implement terms bucket**
```rust
use crate::aggregations::agg_trait::*;
use crate::aggregations::types::{AggregationResult, AggregationValue, Bucket};
use tantivy::{Result as TantivyResult, Searcher, fastfield::FastValueReader};
use std::collections::{HashMap, BinaryHeap};
use std::cmp::Reverse;

pub struct TermsAgg {
    field: String,
    size: usize,
}

pub struct TermsPrepared {
    field: String,
    size: usize,
}

pub struct TermsSegment<T> {
    reader: T,
    field: String,
    counts: HashMap<String, u64>,
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

impl<T: FastValueReader<str>> Agg for TermsAgg {
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
    type Child = TermsSegment<Tantivy::fastfield::FastValueReader<str>>;

    fn create_fruit(&self) -> Self::Fruit {
        TermsFruit(HashMap::new())
    }

    fn for_segment(&self, ctx: &AggSegmentContext) -> TantivyResult<Self::Child> {
        let field = ctx
            .reader
            .fast_fields()
            .str(&self.field)
            .ok_or_else(|| tantivy::TantivyError::SchemaError(format!(
                "field {} not found",
                self.field
            )))?;

        Ok(TermsSegment {
            reader: field,
            field: self.field.clone(),
            counts: HashMap::new(),
        })
    }

    fn merge(&self, acc: &mut Self::Fruit, fruit: Self::Fruit) {
        for (term, count) in fruit.0 {
            *acc.0.entry(term.clone()).or_insert(0) += count;
        }
    }
}

impl<T: FastValueReader<str>> SegmentAgg for TermsSegment<T> {
    type Fruit = TermsFruit;

    fn create_fruit(&self) -> Self::Fruit {
        TermsFruit(self.counts.clone())
    }

    fn collect(&mut self, doc: DocId, _: Score, fruit: &mut Self::Fruit) {
        if let Some(term) = self.reader.get(doc) {
            *fruit.0.entry(term.to_string()).or_insert(0) += 1;
        }
    }
}

impl TermsAgg {
    pub fn into_result(name: String, fruit: TermsFruit) -> AggregationResult {
        // Use BinaryHeap for top-k
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

        // Sort by count descending
        buckets.sort_by(|a, b| b.doc_count.cmp(&a.doc_count));

        AggregationResult {
            name,
            value: AggregationValue::Buckets(buckets),
        }
    }
}
```

**Step 5.2: Create mod.rs for buckets**
```rust
mod terms;

pub use terms::TermsAgg;
```

**Step 5.3: Verify compilation**
Run: `cargo check -p prism`
Expected: Compiles without errors

**Step 5.4: Commit**
```bash
git add prism/src/aggregations/bucket
git commit -m "feat(aggregations): implement terms bucket aggregation"
```

---

## Task 6: Integrate with TextBackend

**Files:**
- Modify: `prism/src/backends/text.rs`

**Step 6.1: Add aggregation request to search**
```rust
// Add to SearchBackend trait
pub trait SearchBackend {
    // ... existing methods ...

    async fn search_with_aggs(
        &self,
        collection: &str,
        query: &Query,
        aggregations: Vec<AggregationRequest>,
    ) -> Result<SearchResultsWithAggs, Error>;
}

#[derive(Debug)]
pub struct SearchResultsWithAggs {
    pub results: Vec<SearchResult>,
    pub total: u64,
    pub aggregations: HashMap<String, AggregationResult>,
}
```

**Step 6.2: Implement in TextBackend**
```rust
use crate::aggregations::*;
use crate::aggregations::types::*;

async fn search_with_aggs(
    &self,
    collection: &str,
    query: &Query,
    aggregations: Vec<AggregationRequest>,
) -> Result<SearchResultsWithAggs, Error> {
    let collections = self.collections.read().unwrap();
    let index = collections
        .get(collection)
        .ok_or_else(|| Error::NotFound(collection.to_string()))?;

    let reader = index.reader.searcher();
    let query_parser = QueryParser::for_index(&index.schema);
    let parsed_query = query_parser.parse_query(&query.query_string)?;

    let searcher = index.reader.searcher();
    let top_docs = TopDocs::with_limit(query.limit as usize);

    // Execute aggregations
    let mut agg_results = HashMap::new();
    for agg_req in aggregations {
        let result = match &agg_req.agg_type {
            AggregationType::Count => {
                let agg = CountAgg;
                let prepared = agg.prepare(&searcher)?;
                let fruit = run_aggregation(&reader, &parsed_query, &top_docs, prepared)?;
                CountAgg::into_result(agg_req.name.clone(), fruit)
            }
            AggregationType::Min { field } => {
                let agg = MinMaxAgg::min(field);
                let prepared = agg.prepare(&searcher)?;
                let fruit = run_aggregation(&reader, &parsed_query, &top_docs, prepared)?;
                MinMaxAgg::into_result(agg_req.name.clone(), fruit)
            }
            AggregationType::Max { field } => {
                let agg = MinMaxAgg::max(field);
                let prepared = agg.prepare(&searcher)?;
                let fruit = run_aggregation(&reader, &parsed_query, &top_docs, prepared)?;
                MinMaxAgg::into_result(agg_req.name.clone(), fruit)
            }
            AggregationType::Terms { field, size } => {
                let agg = TermsAgg::new(field, size.unwrap_or(10));
                let prepared = agg.prepare(&searcher)?;
                let fruit = run_aggregation(&reader, &parsed_query, &top_docs, prepared)?;
                TermsAgg::into_result(agg_req.name.clone(), fruit)
            }
            _ => return Err(Error::Invalid(format!("Unsupported aggregation type"))),
        };

        agg_results.insert(agg_req.name, result);
    }

    // Get search results
    let mut results = Vec::new();
    for (score, doc_address) in searcher.search(&parsed_query, &top_docs)? {
        let doc = reader.doc(doc_address)?;
        results.push(SearchResult {
            id: extract_id(&doc)?,
            score,
            fields: extract_fields(&doc)?,
        });
    }

    Ok(SearchResultsWithAggs {
        results,
        total: results.len() as u64,
        aggregations: agg_results,
    })
}

fn run_aggregation<F: PreparedAgg>(
    reader: &IndexReader,
    query: &tantivy::query::Query,
    top_docs: &TopDocs,
    prepared: F,
) -> Result<F::Fruit, Error>
where
    F::Child: SegmentAgg<Fruit = F::Fruit>,
{
    let mut fruit = prepared.create_fruit();

    for segment_reader in reader.segment_readers() {
        let segment_ord = segment_reader.segment_id();
        let scorer = query.scorer(segment_reader, Boost::default())?;

        let mut segment_agg = prepared.for_segment(&AggSegmentContext {
            segment_ord,
            reader: segment_reader,
            scorer: scorer.as_ref(),
        })?;

        for (doc, score) in segment_reader.search(query, &top_docs)? {
            segment_agg.collect(doc, score, &mut fruit);
        }

        let segment_fruit = segment_agg.create_fruit();
        prepared.merge(&mut fruit, segment_fruit);
    }

    Ok(fruit)
}
```

**Step 6.3: Verify compilation**
Run: `cargo check -p prism`
Expected: Compiles without errors

**Step 6.4: Commit**
```bash
git add prism/src/backends/text.rs prism/src/backends/trait.rs
git commit -m "feat(aggregations): integrate aggregations with TextBackend"
```

---

## Summary

| Task | Description | Files | Est. Time |
|------|-------------|-------|-----------|
| 1 | Create Core Aggregation Module | aggregations/mod.rs, types.rs, lib.rs | 10 min |
| 2 | Three-Tier Trait System | aggregations/agg_trait.rs | 15 min |
| 3 | Count Aggregation | metric/count.rs, mod.rs | 10 min |
| 4 | Min/Max Aggregation | metric/minmax.rs | 20 min |
| 5 | Terms Bucket Aggregation | bucket/terms.rs, mod.rs | 25 min |
| 6 | TextBackend Integration | backends/text.rs, trait.rs | 20 min |

**Total: ~1.5 hours**

---

## Dependencies Graph

```
Task 1 (core types)
    ├── Task 2 (trait system)
    │       ├── Task 3 (count)
    │       ├── Task 4 (min/max)
    │       └── Task 5 (terms)
    │               └── Task 6 (backend integration)
```
