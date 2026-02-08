export interface SearchResult {
  id: string
  title: string
  url: string
  displayDomain: string
  snippet: string
  publishedAt?: string
  faviconUrl?: string
  score: number
}

export interface AnswerModel {
  shortAnswer: string
  keyPoints: string[]
  concepts: string[]
  citations: Record<number, string[]> // keyPoint index → source IDs
  followUps: string[]
  caveats: string[]
  confidence: 'low' | 'medium' | 'high'
}

export type Intent = 'chat' | 'search'
export type LoadingPhase = 'understanding' | 'searching' | 'synthesizing'

export interface SearchState {
  view: 'home' | 'loading' | 'results'
  query: string
  intent: Intent
  intentOverride: Intent | null
  phase: LoadingPhase | null
  results: SearchResult[] | null
  discussions: SearchResult[] | null
  answer: AnswerModel | null
}
