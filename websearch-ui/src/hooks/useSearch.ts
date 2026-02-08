import { useState, useCallback, useEffect } from 'react'
import type { SearchState, LoadingPhase, Intent, SearchResult, AnswerModel } from '@/lib/types'
import { classifyIntent } from '@/lib/intent'
import { search } from '@/lib/api'

const LOADING_PHASES: { key: LoadingPhase; duration: number }[] = [
  { key: 'understanding', duration: 300 },
  { key: 'searching', duration: 1000 },
  { key: 'synthesizing', duration: 200 },
]

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

  // Update URL when query changes
  useEffect(() => {
    if (state.view === 'results' && state.query) {
      const url = new URL(window.location.href)
      url.searchParams.set('q', state.query)
      window.history.replaceState({}, '', url.toString())
    } else if (state.view === 'home') {
      const url = new URL(window.location.href)
      url.searchParams.delete('q')
      window.history.replaceState({}, '', url.toString())
    }
  }, [state.view, state.query])

  // Check URL on mount for initial query
  useEffect(() => {
    const url = new URL(window.location.href)
    const q = url.searchParams.get('q')
    if (q) {
      search(q)
    }
  }, [])

  const search = useCallback(async (query: string) => {
    if (!query.trim()) return

    const intent = classifyIntent(query)

    setState((s) => ({
      ...s,
      view: 'loading',
      query,
      intent,
      phase: 'understanding',
      results: null,
      discussions: null,
      answer: null,
    }))

    // Progress through loading phases
    let phaseIndex = 0
    const phaseInterval = setInterval(() => {
      phaseIndex++
      if (phaseIndex < LOADING_PHASES.length) {
        setState((s) => ({
          ...s,
          phase: LOADING_PHASES[phaseIndex].key,
        }))
      }
    }, LOADING_PHASES[phaseIndex].duration)

    try {
      const data = await search(query, 10)

      clearInterval(phaseInterval)

      const mappedResults: SearchResult[] = data.results.map(r => ({
        id: r.id,
        title: r.title || 'Untitled',
        url: r.url || '#',
        displayDomain: r.url ? new URL(r.url).hostname : 'unknown',
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
      clearInterval(phaseInterval)
      console.error('Search failed:', error)
      setState(initialState)
    }
  }, [])

  const setIntentOverride = useCallback((override: Intent | null) => {
    setState((s) => ({ ...s, intentOverride: override }))
  }, [])

  const reset = useCallback(() => {
    setState(initialState)
  }, [])

  const effectiveIntent = state.intentOverride ?? state.intent

  return {
    ...state,
    effectiveIntent,
    search,
    setIntentOverride,
    reset,
  }
}
