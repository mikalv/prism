import type { SearchResult } from '@/lib/types'
import { TopBar } from './TopBar'
import { ResultsList } from './ResultsList'

interface SearchPageProps {
  query: string
  results: SearchResult[] | null
  onNewSearch: (query: string) => void
}

export function SearchPage({ query, results, onNewSearch }: SearchPageProps) {
  return (
    <div className="min-h-screen flex flex-col">
      <TopBar query={query} onSearch={onNewSearch} />

      <main className="flex-1 px-6 py-6">
        <div className="max-w-4xl mx-auto">
          {results && results.length > 0 ? (
            <ResultsList results={results} variant="full" />
          ) : results && results.length === 0 ? (
            <div className="text-center py-12">
              <p className="text-[var(--text-secondary)]">
                No results found for "{query}"
              </p>
            </div>
          ) : null}
        </div>
      </main>
    </div>
  )
}
