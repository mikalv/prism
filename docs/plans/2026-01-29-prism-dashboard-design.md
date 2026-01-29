# Prism Dashboard Design

**Date:** 2026-01-29
**Status:** Approved
**Related Issues:** #20 (Dashboard), #14 (Index Viewer)

## Overview

A Kibana-like dashboard for Prism to explore indices, documents, mappings, queries, aggregations, and cache stats. Merges functionality from both issue #20 (Dashboard) and issue #14 (Index Viewer) into a unified tool.

## Goals

- Visual inspector for collections, schemas, and field mappings
- Search UI with query DSL editor and saved queries
- Aggregations explorer (buckets + metrics) with charts
- Document viewer with pagination and source display
- Index inspection: top terms, segments, document reconstruction
- Cache/health stats: hits/misses, storage stats, index info
- DevTools panel for debugging API requests

## Non-Goals (MVP)

- Write/update documents
- Index management operations (create/delete collections)
- Authentication/authorization

## Tech Stack

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Framework | Vite + React 18 + TypeScript | Fast dev, familiar to team |
| Styling | TailwindCSS + shadcn/ui | Copy-paste components, no lock-in |
| Data Fetching | TanStack Query | Caching, refetching, loading states |
| Charts | Recharts | React-native, declarative API |
| Query Editor | Monaco Editor | VS Code experience, syntax highlighting |
| Location | `prism/dashboard/` | Co-located with backend |

## Project Structure

```
prism/
├── dashboard/
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── tailwind.config.js
│   ├── index.html
│   ├── src/
│   │   ├── main.tsx
│   │   ├── App.tsx
│   │   ├── api/
│   │   │   ├── client.ts          # Fetch wrapper with DevTools logging
│   │   │   └── types.ts           # API response types
│   │   ├── components/
│   │   │   ├── ui/                # shadcn/ui components
│   │   │   ├── Layout.tsx         # Sidebar + main content
│   │   │   ├── DevTools.tsx       # Request/response panel
│   │   │   └── ConnectionStatus.tsx
│   │   ├── pages/
│   │   │   ├── Collections.tsx
│   │   │   ├── Search.tsx
│   │   │   ├── Aggregations.tsx
│   │   │   ├── Index.tsx
│   │   │   └── Stats.tsx
│   │   ├── hooks/
│   │   │   ├── useCollections.ts
│   │   │   ├── useSearch.ts
│   │   │   └── useStats.ts
│   │   └── lib/
│   │       └── utils.ts
│   └── .env.example               # VITE_PRISM_URL=http://localhost:3080
├── prism/                         # Existing backend
├── prism-server/
└── prism-cli/
```

## UI Layout

```
┌─────────────────────────────────────────────────────────────┐
│  Prism Dashboard                       [server: connected]  │
├─────────────┬───────────────────────────────────────────────┤
│             │                                               │
│  Collections│   (Main content based on selected section)    │
│  Search     │                                               │
│  Aggregations                                               │
│  Index      │                                               │
│  Stats      │                                               │
│             │                                               │
│─────────────│                                               │
│  Connection │                                               │
│  localhost  │                                               │
│  :3080      │                                               │
├─────────────┴───────────────────────────────────────────────┤
│  DevTools (collapsible)                      [▲ Collapse]   │
└─────────────────────────────────────────────────────────────┘
```

## Pages

### Collections Page

Displays all collections with expandable schema details.

**Collapsed view:**
- Collection name
- Document count
- Storage size
- Vector dimensions (if applicable)

**Expanded view:**
- Field table: name, type, indexed, vector source
- Quick actions: Jump to Search/Aggregations

### Search Page

Full-featured query interface.

**Components:**
- Collection selector dropdown
- Monaco Editor for query input (min 4 lines, resizable)
- Query settings panel:
  - Fields multiselect (from schema)
  - Limit/offset inputs
  - Search strategy: Text / Vector / Hybrid
  - Weight sliders for hybrid mode
- Results table with score, fields, actions
- Pagination controls
- Save/load query (localStorage)

**Keyboard shortcuts:**
- `Cmd+Enter` - Execute search

### Aggregations Page

Build and visualize aggregations.

**Aggregation builder:**
- Field selector
- Type: Terms, Date Histogram, Range, Stats
- Size (for Terms)
- Interval (for Date Histogram)
- Optional filter query
- Add multiple aggregations

**Visualization:**
| Type | Chart |
|------|-------|
| Terms | Horizontal bar chart |
| Date Histogram | Line/area chart |
| Range | Stacked bar |
| Stats | Metric cards (min/max/avg/sum) |

**Export:** JSON, CSV

### Index Page (from Issue #14)

Low-level index inspection for debugging.

**Tabs:**

1. **Top Terms**
   - Field selector
   - Limit input
   - Table: term, doc frequency, total frequency, percentage
   - Word cloud visualization option

2. **Segments**
   - Table: segment name, docs, deleted, size, created
   - Delete ratio indicator (healthy < 10%)
   - Size distribution bar chart

3. **Document Lookup**
   - Document ID input
   - Stored fields display
   - Indexed terms per field (as tags)
   - Vector preview (first N dimensions + magnitude)
   - Actions: Copy JSON, Find Similar

### Stats Page

System health and performance metrics.

**Sections:**

1. **Embedding Cache**
   - Metric cards: entries, hits, misses, hit rate
   - Storage usage bar
   - Hit rate time series (last hour, collected in frontend)

2. **Collections Storage**
   - Table: collection, docs, text index size, vector index size
   - Totals row

3. **Server Info**
   - Version, uptime, OS
   - Embedding provider details

**Auto-refresh:** Toggle 5s/10s/30s/off

### DevTools Panel

Debugging panel accessible from all pages.

**Features:**
- Request/response viewer (JSON with Monaco)
- Request history with timing
- Replay button
- Copy as cURL / Copy as fetch
- Clear history

**Toggle:** `Cmd+Shift+D`

## Backend API Requirements

### Existing Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /collections` | List collection names |
| `POST /:collection/search` | Search with query DSL |
| `GET /:collection/:id` | Get document by ID |

### New Endpoints Required

| Endpoint | Description | Priority |
|----------|-------------|----------|
| `GET /:collection/schema` | Field types, indexing config | High |
| `GET /:collection/stats` | Doc count, storage size | High |
| `GET /stats/cache` | Cache hits/misses/entries/bytes | High |
| `GET /stats/server` | Version, uptime, provider | Medium |
| `POST /:collection/aggregate` | Run aggregations | High |
| `GET /:collection/terms/:field` | Top-k terms | Medium |
| `GET /:collection/segments` | Segment statistics | Medium |
| `GET /:collection/doc/:id/reconstruct` | Full reconstruction | Medium |

### CORS Support

Dashboard runs on different port than Prism server. Add CORS middleware:

```rust
// prism/src/api/server.rs
use tower_http::cors::CorsLayer;

// In router setup:
.layer(CorsLayer::permissive()) // Dev mode
// Or configurable whitelist for production
```

## Implementation Phases

### Phase 1: Foundation
- [ ] Create `dashboard/` with Vite + React + TypeScript
- [ ] Install shadcn/ui, TailwindCSS, TanStack Query, Recharts, Monaco
- [ ] Build API client with DevTools logging
- [ ] Implement layout with sidebar + connection indicator
- [ ] Backend: Add CORS support to Prism

### Phase 2: Collections + Schema
- [ ] Backend: `GET /:collection/schema` and `GET /:collection/stats`
- [ ] Frontend: Collections page with schema display
- [ ] Expandable collection cards with field types

### Phase 3: Search
- [ ] Monaco Editor integration
- [ ] Search page with query settings panel
- [ ] Results table with pagination
- [ ] Saved queries (localStorage)

### Phase 4: Stats
- [ ] Backend: `GET /stats/cache` and `GET /stats/server`
- [ ] Frontend: Stats page with metric cards
- [ ] Hit rate time series chart
- [ ] Auto-refresh toggle

### Phase 5: Aggregations
- [ ] Backend: `POST /:collection/aggregate`
- [ ] Frontend: Aggregation builder UI
- [ ] Charts: Bar (terms), Line (date histogram), Stats cards

### Phase 6: Index Inspector
- [ ] Backend: Terms, segments, reconstruct endpoints
- [ ] Frontend: Index page with 3 tabs
- [ ] Top terms table + word cloud
- [ ] Segment view with size distribution

### Phase 7: Polish
- [ ] Dark/light mode (system preference)
- [ ] Keyboard shortcuts (Cmd+K search, Cmd+Shift+D devtools)
- [ ] Error handling and loading states
- [ ] README and documentation

## GitHub Issues to Create

1. **Collection Metadata API** - `GET /:collection/schema`, `GET /:collection/stats`
2. **Cache Stats API** - `GET /stats/cache`, `GET /stats/server`
3. **Aggregations API** - `POST /:collection/aggregate`
4. **Index Inspection API** - terms, segments, reconstruct endpoints
5. **CORS Support** - Add CORS middleware to Prism server

## Success Criteria

- [ ] Can browse all collections and their schemas
- [ ] Can execute searches and see results with timing
- [ ] Can run aggregations and visualize as charts
- [ ] Can inspect index internals (top terms, segments)
- [ ] Can view cache hit/miss statistics
- [ ] DevTools shows all API requests for debugging
- [ ] Works on localhost development setup
