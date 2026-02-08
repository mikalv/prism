import type { SearchResult } from '@/lib/types'
import { ResultCard } from './ResultCard'

interface DiscussionsListProps {
  discussions: SearchResult[]
}

export function DiscussionsList({ discussions }: DiscussionsListProps) {
  if (!discussions.length) return null

  return (
    <div className="flex flex-col gap-3 mt-6">
      <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide">
        Discussions
      </h2>
      {discussions.map((result) => (
        <ResultCard key={result.id} result={result} />
      ))}
    </div>
  )
}
