import { useState, useCallback, useEffect, useRef } from 'react'
import type { SearchState, SearchResult } from '@/lib/types'
import { multiSearch } from '@/lib/api'

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
  const [collections, setCollections] = useState<string[]>([])

  const isMounted = useRef(false)

  // Update URL when query changes
  useEffect(() => {
    if (!isMounted.current) {
      isMounted.current = true
      return
    }

    if (state.view === 'results' && state.query) {
      const url = new URL(window.location.href)
      url.searchParams.set('q', state.query)
      url.searchParams.delete('c')
      if (collections.length > 0) {
        collections.forEach(col => url.searchParams.append('c', col))
      }
      window.history.replaceState({}, '', url.toString())
    } else if (state.view === 'home') {
      const url = new URL(window.location.href)
      url.searchParams.delete('q')
      url.searchParams.delete('c')
      window.history.replaceState({}, '', url.toString())
    }
  }, [state.view, state.query, collections])

  // Check URL on mount for initial query
  useEffect(() => {
    const url = new URL(window.location.href)
    const q = url.searchParams.get('q')
    const c = url.searchParams.getAll('c')
    if (q) {
      doSearch(q, c)
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const doSearch = useCallback(async (query: string, cols: string[] = []) => {
    if (!query.trim()) return

    setCollections(cols)

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
      const data = await multiSearch(cols, query, 20)

      const mappedResults: SearchResult[] = data.results.map((r) => ({
        id: r.id,
        title: r.title || r.id,
        url: r.url || '#',
        displayDomain: r.url ? new URL(r.url).hostname : '',
        snippet: r.snippet || '',
        score: r.score,
        collection: r.collection,
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
    setCollections([])
  }, [])

  return {
    ...state,
    collections,
    effectiveIntent: 'search' as const,
    search: doSearch,
    setIntentOverride: () => {},
    reset,
  }
}

