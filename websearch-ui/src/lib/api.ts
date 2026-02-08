const API_BASE_URL = import.meta.env.VITE_API_URL || 'http://localhost:3080';

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
