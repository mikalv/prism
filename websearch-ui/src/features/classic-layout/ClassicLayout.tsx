import { HomePage } from '@/features/home'
import { LoadingPage } from '@/features/loading'
import { SearchPage } from '@/features/search'
import type { useSearch } from '@/hooks'

interface ClassicLayoutProps {
  search: ReturnType<typeof useSearch>
}

export function ClassicLayout({ search }: ClassicLayoutProps) {
  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)]">
      {search.view === 'home' && <HomePage onSearch={search.search} />}

      {search.view === 'loading' && (
        <LoadingPage query={search.query} phase={search.phase} />
      )}

      {search.view === 'results' && (
        <SearchPage
          query={search.query}
          effectiveIntent={search.effectiveIntent}
          results={search.results}
          discussions={search.discussions}
          answer={search.answer}
          onNewSearch={search.search}
          setIntentOverride={search.setIntentOverride}
        />
      )}
    </div>
  )
}
