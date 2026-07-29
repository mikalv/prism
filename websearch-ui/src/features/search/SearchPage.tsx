import type { SearchResult } from '@/lib/types'
import { TopBar } from './TopBar'
import { ResultsList } from './ResultsList'
import { Database, CheckSquare, Square } from 'lucide-react'
import { useEffect, useState } from 'react'
import { getCollections } from '@/lib/api'

interface SearchPageProps {
  query: string
  collections: string[]
  results: SearchResult[] | null
  onNewSearch: (query: string, collections?: string[]) => void
}

export function SearchPage({ query, collections, results, onNewSearch }: SearchPageProps) {
  const [allCollections, setAllCollections] = useState<string[]>([])

  useEffect(() => {
    getCollections().then(setAllCollections)
  }, [])

  const toggleCollection = (col: string) => {
    let newCols = [...collections]
    if (newCols.includes(col)) {
      newCols = newCols.filter((c) => c !== col)
    } else {
      newCols.push(col)
    }
    onNewSearch(query, newCols)
  }

  const clearCollections = () => {
    onNewSearch(query, [])
  }

  return (
    <div className="min-h-screen flex flex-col">
      <TopBar query={query} onSearch={(q) => onNewSearch(q, collections)} />

      <main className="flex-1 px-6 py-6 flex gap-8 max-w-6xl mx-auto w-full">
        {/* Sidebar / Facets */}
        <aside className="w-64 hidden md:block shrink-0">
          <div className="sticky top-24 space-y-6">
            <div>
              <h3 className="font-semibold text-[var(--text-primary)] mb-3 flex items-center gap-2">
                <Database className="w-4 h-4" />
                Collections
              </h3>
              <div className="space-y-1">
                <button
                  onClick={clearCollections}
                  className={`
                    w-full px-3 py-1.5 text-left text-sm rounded-md flex items-center gap-2
                    hover:bg-[var(--bg-tertiary)] transition-colors
                    ${collections.length === 0 ? 'text-[var(--accent)] font-medium' : 'text-[var(--text-secondary)]'}
                  `}
                >
                  <Database className="w-4 h-4 opacity-70" />
                  All collections
                </button>
                {allCollections.map(col => {
                  const isSelected = collections.includes(col)
                  return (
                    <button
                      key={col}
                      onClick={() => toggleCollection(col)}
                      className={`
                        w-full px-3 py-1.5 text-left text-sm rounded-md flex items-center gap-2 truncate
                        hover:bg-[var(--bg-tertiary)] transition-colors
                        ${isSelected ? 'text-[var(--accent)] font-medium' : 'text-[var(--text-secondary)]'}
                      `}
                      title={col}
                    >
                      {isSelected ? <CheckSquare className="w-4 h-4" /> : <Square className="w-4 h-4 opacity-50" />}
                      {col}
                    </button>
                  )
                })}
              </div>
            </div>
          </div>
        </aside>

        <div className="flex-1 min-w-0">
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

