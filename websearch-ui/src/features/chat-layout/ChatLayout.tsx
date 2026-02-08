import { useState, useEffect } from 'react'
import { ChatHomePage } from './ChatHomePage'
import { ChatLoadingPage } from './ChatLoadingPage'
import { ChatResultsPage } from './ChatResultsPage'
import type { useSearch } from '@/hooks'

type SearchMode = 'search' | 'deepThink'

interface ChatLayoutProps {
  search: ReturnType<typeof useSearch>
}

export function ChatLayout({ search }: ChatLayoutProps) {
  const [mode, setMode] = useState<SearchMode>('search')
  const [inputValue, setInputValue] = useState('')

  const handleSubmit = () => {
    if (inputValue.trim()) {
      search.search(inputValue.trim())
    }
  }

  // Sync input with current query when on results page
  useEffect(() => {
    if (search.view === 'results' && search.query) {
      setInputValue(search.query)
    }
  }, [search.view, search.query])

  // Clear input when going back to home
  useEffect(() => {
    if (search.view === 'home') {
      setInputValue('')
    }
  }, [search.view])

  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)]">
      {search.view === 'home' && (
        <ChatHomePage
          value={inputValue}
          onChange={setInputValue}
          onSubmit={handleSubmit}
          mode={mode}
          onModeChange={setMode}
        />
      )}

      {search.view === 'loading' && (
        <ChatLoadingPage query={search.query} phase={search.phase} />
      )}

      {search.view === 'results' && (
        <ChatResultsPage
          query={search.query}
          effectiveIntent={search.effectiveIntent}
          results={search.results}
          discussions={search.discussions}
          answer={search.answer}
          value={inputValue}
          onChange={setInputValue}
          onSubmit={handleSubmit}
          searchMode={mode}
          onModeChange={setMode}
        />
      )}
    </div>
  )
}
