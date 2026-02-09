import { useState, useCallback, useEffect } from 'react'
import type { SearchState, SearchResult } from '@/lib/types'
import { search as apiSearch, searchCollection } from '@/lib/api'

const initialState: SearchState = {
  view: 'home',
  query: '',
  intent: 'search',
  intentOverride: null,
  phase: null,
  results: null,
  discussions: null,
  answer: null,
}

export function useSearch() {
  const [state, setState] = useState<SearchState>(initialState)
  const [collection, setCollection] = useState<string | undefined>(undefined)

  // Update URL when query changes
  useEffect(() => {
    if (state.view === 'results' && state.query) {
      const url = new URL(window.location.href)
      url.searchParams.set('q', state.query)
      if (collection) {
        url.searchParams.set('c', collection)
      } else {
        url.searchParams.delete('c')
      }
      window.history.replaceState({}, '', url.toString())
    } else if (state.view === 'home') {
      const url = new URL(window.location.href)
      url.searchParams.delete('q')
      url.searchParams.delete('c')
      window.history.replaceState({}, '', url.toString())
    }
  }, [state.view, state.query, collection])

  // Check URL on mount for initial query
  useEffect(() => {
    const url = new URL(window.location.href)
    const q = url.searchParams.get('q')
    const c = url.searchParams.get('c') || undefined
    if (q) {
      doSearch(q, c)
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const doSearch = useCallback(async (query: string, col?: string) => {
    if (!query.trim()) return

    setCollection(col)

    setState((s) => ({
      ...s,
      view: 'loading',
      query,
      intent: 'search',
      phase: 'searching',
      results: null,
      discussions: null,
      answer: null,
    }))

    try {
      // Use collection-specific search if collection is selected
      const data = col
        ? await searchCollection(col, query, 20)
        : await apiSearch(query, 20)

      const mappedResults: SearchResult[] = data.results.map((r) => ({
        id: r.id,
        title: r.title || r.id,
        url: r.url || '#',
        displayDomain: r.url ? new URL(r.url).hostname : '',
        snippet: r.snippet || '',
        score: r.score,
      }))

      setState((s) => ({
        ...s,
        view: 'results',
        phase: null,
        results: mappedResults,
        discussions: [],
        answer: null,
      }))
    } catch (error) {
      console.error('Search failed:', error)
      setState((s) => ({
        ...s,
        view: 'results',
        phase: null,
        results: [],
        discussions: [],
        answer: null,
      }))
    }
  }, [])

  const reset = useCallback(() => {
    setState(initialState)
    setCollection(undefined)
  }, [])

  return {
    ...state,
    collection,
    effectiveIntent: 'search' as const,
    search: doSearch,
    setIntentOverride: () => {},
    reset,
  }
}
