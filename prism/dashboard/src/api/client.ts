const API_BASE = import.meta.env.VITE_API_URL || 'http://localhost:3000'

async function fetchApi<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  })
  if (!res.ok) {
    throw new Error(`API error: ${res.status} ${res.statusText}`)
  }
  return res.json()
}

export interface ServerInfo {
  version: string
  name: string
}

export interface CacheStats {
  hits: number
  misses: number
  hit_rate: number
}

export interface CollectionStats {
  document_count: number
  storage_size_bytes: number
}

export interface FieldSchema {
  name: string
  field_type: string
  indexed: boolean
  stored: boolean
  vector_dimensions?: number
}

export interface CollectionSchema {
  fields: FieldSchema[]
}

export interface AggregationBucket {
  key: string
  doc_count: number
}

export interface AggregationResult {
  buckets: AggregationBucket[]
}

export interface SearchHit {
  id?: string
  score: number
  document: Record<string, unknown>
}

export interface SearchResults {
  hits: SearchHit[]
  total: number
}

export const api = {
  getServerInfo: () => fetchApi<ServerInfo>('/stats/server'),
  getCacheStats: () => fetchApi<CacheStats>('/stats/cache'),
  listCollections: () => fetchApi<string[]>('/collections'),
  getCollectionSchema: (collection: string) =>
    fetchApi<CollectionSchema>(`/collections/${collection}/schema`),
  getCollectionStats: (collection: string) =>
    fetchApi<CollectionStats>(`/collections/${collection}/stats`),
  runAggregation: (collection: string, field: string, size = 10) =>
    fetchApi<AggregationResult>(`/collections/${collection}/aggregate`, {
      method: 'POST',
      body: JSON.stringify({
        aggregations: {
          terms: { field, size },
        },
      }),
    }),
  search: (collection: string, query: string, limit = 10) =>
    fetchApi<SearchResults>(`/collections/${collection}/search`, {
      method: 'POST',
      body: JSON.stringify({ query, limit }),
    }),
}
