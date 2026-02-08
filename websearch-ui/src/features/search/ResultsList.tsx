import type { SearchResult } from '@/lib/types'
import { ResultCard } from './ResultCard'

interface ResultsListProps {
  results: SearchResult[]
  variant?: 'full' | 'compact'
}

export function ResultsList({ results, variant = 'full' }: ResultsListProps) {
  const displayResults = variant === 'compact' ? results.slice(0, 4) : results

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide">
        Web Results
      </h2>
      {displayResults.map((result) => (
        <ResultCard key={result.id} result={result} compact={variant === 'compact'} />
      ))}
    </div>
  )
}
