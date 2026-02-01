# Aggregations

Aggregations compute analytics over your search results or entire collections. Prism supports metric aggregations (single values), bucket aggregations (grouping), and nested sub-aggregations.

## Endpoint

```
POST /collections/:collection/aggregate
```

### Request format

```json
{
  "query": "optional filter query",
  "scan_limit": 10000,
  "aggregations": [
    {
      "name": "my_agg",
      "type": "terms",
      "field": "category",
      "size": 10
    }
  ]
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `query` | string | `"*"` (all docs) | Optional filter query |
| `scan_limit` | integer | 10000 | Max documents to scan |
| `aggregations` | array | required | List of aggregation definitions |

Each aggregation requires:
- `name` — unique identifier for this aggregation in the response
- `type` — aggregation type (see below)
- Type-specific fields (e.g., `field`, `interval`, `ranges`)

### Response format

```json
{
  "results": {
    "my_agg": {
      "name": "my_agg",
      ...aggregation-specific value...
    }
  },
  "took_ms": 12
}
```

---

## Metric aggregations

Metric aggregations compute a single numeric value from a field.

### Count

Count all matching documents:

```json
{ "name": "total", "type": "count" }
```

Response: `{ "name": "total", 42.0 }`

### Sum

```json
{ "name": "total_revenue", "type": "sum", "field": "price" }
```

### Avg

```json
{ "name": "avg_price", "type": "avg", "field": "price" }
```

### Min / Max

```json
{ "name": "cheapest", "type": "min", "field": "price" }
{ "name": "most_expensive", "type": "max", "field": "price" }
```

### Stats

Compute count, min, max, sum, and average in one aggregation:

```json
{ "name": "price_stats", "type": "stats", "field": "price" }
```

Response:

```json
{
  "name": "price_stats",
  "count": 1000,
  "min": 4.99,
  "max": 999.99,
  "sum": 125430.50,
  "avg": 125.43
}
```

### Percentiles

Compute specific percentile values for distribution analysis:

```json
{
  "name": "latency_percentiles",
  "type": "percentiles",
  "field": "response_time",
  "percents": [50, 95, 99]
}
```

Response:

```json
{
  "name": "latency_percentiles",
  "values": {
    "50": 12.5,
    "95": 42.0,
    "99": 128.7
  }
}
```

Default percentiles if `percents` is omitted: `[1, 5, 25, 50, 75, 95, 99]`.

The implementation uses sorted-array linear interpolation for exact computation.

---

## Bucket aggregations

Bucket aggregations group documents into buckets and count documents per bucket.

### Terms

Group by unique field values:

```json
{
  "name": "categories",
  "type": "terms",
  "field": "category",
  "size": 10
}
```

Response:

```json
{
  "name": "categories",
  [
    { "key": "electronics", "doc_count": 450 },
    { "key": "books", "doc_count": 320 },
    { "key": "clothing", "doc_count": 180 }
  ]
}
```

Works with both string and numeric fields. Results are sorted by `doc_count` descending.

### Histogram

Group numeric values into fixed-width buckets:

```json
{
  "name": "price_distribution",
  "type": "histogram",
  "field": "price",
  "interval": 50,
  "min_doc_count": 1,
  "extended_bounds": { "min": 0, "max": 500 }
}
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `interval` | float | required | Bucket width |
| `min_doc_count` | integer | 0 | Minimum docs to include bucket |
| `extended_bounds` | object | null | Force bucket range `{ min, max }` |

Response:

```json
[
  { "key": "0", "doc_count": 15 },
  { "key": "50", "doc_count": 42 },
  { "key": "100", "doc_count": 38 },
  { "key": "150", "doc_count": 20 },
  { "key": "200", "doc_count": 8 }
]
```

With `extended_bounds`, empty buckets are included within the specified range. Without it, only buckets with documents are returned.

### Date histogram

Group timestamps into calendar-aligned buckets:

```json
{
  "name": "orders_over_time",
  "type": "date_histogram",
  "field": "created_at",
  "calendar_interval": "month",
  "min_doc_count": 0
}
```

| Interval | Description |
|----------|-------------|
| `hour` | Rounded to hour boundary |
| `day` | Rounded to day boundary |
| `week` | Rounded to Monday |
| `month` | Rounded to 1st of month |
| `year` | Rounded to January 1st |

Timestamp fields can be stored as:
- `date` type (Tantivy native)
- `i64` or `u64` (microseconds since Unix epoch)

Response buckets are sorted chronologically with RFC 3339 keys:

```json
[
  { "key": "2025-01-01T00:00:00+00:00", "doc_count": 120 },
  { "key": "2025-02-01T00:00:00+00:00", "doc_count": 145 },
  { "key": "2025-03-01T00:00:00+00:00", "doc_count": 98 }
]
```

### Range

Group by custom numeric boundaries:

```json
{
  "name": "price_ranges",
  "type": "range",
  "field": "price",
  "ranges": [
    { "key": "cheap", "to": 50 },
    { "key": "mid", "from": 50, "to": 200 },
    { "key": "expensive", "from": 200 }
  ]
}
```

| Range field | Description |
|-------------|-------------|
| `key` | Optional human-readable name. Auto-generated if omitted (e.g., `"50-200"`) |
| `from` | Inclusive lower bound. Omit for unbounded. |
| `to` | Exclusive upper bound. Omit for unbounded. |

Response:

```json
[
  { "key": "cheap", "doc_count": 150, "to": 50.0 },
  { "key": "mid", "doc_count": 420, "from": 50.0, "to": 200.0 },
  { "key": "expensive", "doc_count": 80, "from": 200.0 }
]
```

---

## Filter aggregations

Filter aggregations narrow the aggregation context using a query.

### Single filter

Compute aggregations only on documents matching a filter:

```json
{
  "name": "expensive_items",
  "type": "filter",
  "filter": "price:[100 TO *]",
  "aggs": [
    { "name": "avg_rating", "type": "avg", "field": "rating" }
  ]
}
```

Response:

```json
{
  "name": "expensive_items",
  [
    {
      "key": "filter",
      "doc_count": 250,
      "sub_aggs": [
        { "name": "avg_rating", 4.2 }
      ]
    }
  ]
}
```

### Named filters

Multiple named filters in one aggregation:

```json
{
  "name": "status_breakdown",
  "type": "filters",
  "filters": {
    "active": "status:active",
    "archived": "status:archived",
    "draft": "status:draft"
  },
  "aggs": [
    { "name": "count", "type": "count" }
  ]
}
```

Response:

```json
[
  { "key": "active", "doc_count": 1200, "sub_aggs": [{ "name": "count", 1200.0 }] },
  { "key": "archived", "doc_count": 340, "sub_aggs": [{ "name": "count", 340.0 }] },
  { "key": "draft", "doc_count": 55, "sub_aggs": [{ "name": "count", 55.0 }] }
]
```

---

## Global aggregation

The global aggregation ignores the query filter and runs on all documents in the collection:

```json
{
  "query": "active products",
  "aggregations": [
    {
      "name": "matching_count",
      "type": "count"
    },
    {
      "name": "all_products",
      "type": "global",
      "aggs": [
        { "name": "total_count", "type": "count" },
        { "name": "overall_avg_price", "type": "avg", "field": "price" }
      ]
    }
  ]
}
```

The `matching_count` aggregation runs on query results, while `all_products` runs on the entire collection regardless of the query.

---

## Nested sub-aggregations

Any bucket aggregation can contain sub-aggregations that compute metrics per bucket. Sub-aggregations can themselves be bucket aggregations, enabling multi-level nesting.

### Example: Average price per category

```json
{
  "name": "categories",
  "type": "terms",
  "field": "category",
  "size": 10,
  "aggs": [
    { "name": "avg_price", "type": "avg", "field": "price" },
    { "name": "price_stats", "type": "stats", "field": "price" }
  ]
}
```

Response:

```json
[
  {
    "key": "electronics",
    "doc_count": 450,
    "sub_aggs": [
      { "name": "avg_price", 299.99 },
      { "name": "price_stats", "count": 450, "min": 9.99, "max": 1999.99, "sum": 134995.50, "avg": 299.99 }
    ]
  },
  {
    "key": "books",
    "doc_count": 320,
    "sub_aggs": [
      { "name": "avg_price", 24.50 },
      { "name": "price_stats", "count": 320, "min": 4.99, "max": 89.99, "sum": 7840.00, "avg": 24.50 }
    ]
  }
]
```

### Multi-level nesting: Brands within categories

```json
{
  "name": "categories",
  "type": "terms",
  "field": "category",
  "size": 5,
  "aggs": [
    {
      "name": "top_brands",
      "type": "terms",
      "field": "brand",
      "size": 3,
      "aggs": [
        { "name": "avg_price", "type": "avg", "field": "price" }
      ]
    }
  ]
}
```

### Histogram with sub-aggregations

```json
{
  "name": "price_brackets",
  "type": "histogram",
  "field": "price",
  "interval": 100,
  "aggs": [
    { "name": "avg_rating", "type": "avg", "field": "rating" },
    { "name": "top_categories", "type": "terms", "field": "category", "size": 3 }
  ]
}
```

---

## Complete example

A dashboard analytics query combining multiple aggregation types:

```json
{
  "query": "in_stock:true",
  "aggregations": [
    { "name": "total", "type": "count" },
    { "name": "price_stats", "type": "stats", "field": "price" },
    {
      "name": "price_percentiles",
      "type": "percentiles",
      "field": "price",
      "percents": [25, 50, 75, 90, 99]
    },
    {
      "name": "by_category",
      "type": "terms",
      "field": "category",
      "size": 10,
      "aggs": [
        { "name": "avg_price", "type": "avg", "field": "price" },
        { "name": "avg_rating", "type": "avg", "field": "rating" }
      ]
    },
    {
      "name": "price_histogram",
      "type": "histogram",
      "field": "price",
      "interval": 50,
      "extended_bounds": { "min": 0, "max": 500 }
    },
    {
      "name": "added_over_time",
      "type": "date_histogram",
      "field": "created_at",
      "calendar_interval": "month"
    },
    {
      "name": "price_tiers",
      "type": "range",
      "field": "price",
      "ranges": [
        { "key": "budget", "to": 25 },
        { "key": "mid-range", "from": 25, "to": 100 },
        { "key": "premium", "from": 100, "to": 500 },
        { "key": "luxury", "from": 500 }
      ]
    },
    {
      "name": "all_products",
      "type": "global",
      "aggs": [
        { "name": "total_ever", "type": "count" }
      ]
    }
  ]
}
```
