// API base URL - use relative path when embedded, absolute for dev
const API_BASE_URL = import.meta.env.VITE_API_URL || '';

export interface SimpleSearchRequest {
  query: string;
  limit?: number;
}

export interface SimpleSearchResult {
  id: string;
  title?: string;
  url?: string;
  snippet?: string;
  score: number;
}

export interface SimpleSearchResponse {
  results: SimpleSearchResult[];
  total: number;
}

export async function search(query: string, limit = 10): Promise<SimpleSearchResponse> {
  try {
    const response = await fetch(`${API_BASE_URL}/api/search`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ query, limit }),
    });

    if (!response.ok) {
      throw new Error(`Search failed: ${response.statusText}`);
    }

    return await response.json();
  } catch (error) {
    console.error('Search API error:', error);
    throw error;
  }
}

// Get list of collections
export async function getCollections(): Promise<string[]> {
  try {
    const response = await fetch(`${API_BASE_URL}/admin/collections`);
    if (!response.ok) {
      throw new Error(`Failed to get collections: ${response.statusText}`);
    }
    const data = await response.json();
    return data.collections || [];
  } catch (error) {
    console.error('Collections API error:', error);
    return [];
  }
}

// Search a specific collection
export async function searchCollection(
  collection: string,
  query: string,
  limit = 10
): Promise<SimpleSearchResponse> {
  try {
    const response = await fetch(`${API_BASE_URL}/collections/${collection}/search`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ query, limit }),
    });

    if (!response.ok) {
      throw new Error(`Search failed: ${response.statusText}`);
    }

    const data = await response.json();

    // Transform to SimpleSearchResponse format
    return {
      results: data.results.map((r: Record<string, unknown>) => ({
        id: r.id as string,
        title: (r.fields as Record<string, unknown>)?.title as string || r.id as string,
        url: (r.fields as Record<string, unknown>)?.url as string,
        snippet: r.snippet as string || (r.fields as Record<string, unknown>)?.content as string,
        score: r.score as number,
      })),
      total: data.total,
    };
  } catch (error) {
    console.error('Collection search API error:', error);
    throw error;
  }
}
