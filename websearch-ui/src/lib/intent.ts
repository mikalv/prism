import type { Intent } from './types'

const QUESTION_STARTERS = [
  // Norsk
  'hva',
  'hvordan',
  'hvorfor',
  'hvem',
  'hvor',
  'når',
  'hvilken',
  'kan du',
  'forklar',
  'sammenlign',
  'hjelp',
  'anbefal',
  // Engelsk
  'what',
  'how',
  'why',
  'who',
  'where',
  'when',
  'which',
  'explain',
  'compare',
  'help',
  'recommend',
  'tell me',
]

export function classifyIntent(query: string): Intent {
  const q = query.trim().toLowerCase()
  if (!q) return 'search'

  const tokens = q.split(/\s+/)

  // Eksplisitte søke-operatorer → search
  if (/(site:|filetype:|pdf|github|docs|download|vs\b)/i.test(q)) {
    return 'search'
  }

  // Spørsmålstegn → chat
  if (q.includes('?')) return 'chat'

  // Starter med spørreord → chat
  if (QUESTION_STARTERS.some((s) => q.startsWith(s + ' ') || q === s)) {
    return 'chat'
  }

  // Lange setninger (7+ ord) → chat
  if (tokens.length >= 7) return 'chat'

  // Korte keyword-queries (1-4 ord) → search
  if (tokens.length <= 4) return 'search'

  // Default: chat (bedre UX)
  return 'chat'
}
