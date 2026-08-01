import { useState, KeyboardEvent, useEffect, useRef } from 'react'
import { Search } from 'lucide-react'
import { Input } from '@/components/ui'
import { HeaderActions } from '@/components/composed'

interface TopBarProps {
  query: string
  onSearch: (query: string) => void
}

export function TopBar({ query: initialQuery, onSearch }: TopBarProps) {
  const [query, setQuery] = useState(initialQuery)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const handleKeyDownGlobal = (e: globalThis.KeyboardEvent) => {
      if (e.key === '/' && document.activeElement !== inputRef.current) {
        e.preventDefault()
        inputRef.current?.focus()
      }
    }
    document.addEventListener('keydown', handleKeyDownGlobal)
    return () => document.removeEventListener('keydown', handleKeyDownGlobal)
  }, [])

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && query.trim()) {
      onSearch(query.trim())
    }
  }

  return (
    <header className="sticky top-0 z-10 px-6 py-3 bg-[var(--bg-primary)] border-b border-[var(--border)]">
      <div className="max-w-6xl mx-auto flex items-center gap-4">
        <button
          onClick={() => window.location.reload()}
          className="text-xl font-semibold text-[var(--accent)] hover:opacity-80 transition-opacity"
        >
          WebSearch
        </button>

        <div className="flex-1 max-w-xl">
          <Input
            ref={inputRef}
            size="md"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Search..."
            leftIcon={<Search className="w-4 h-4" />}
          />
        </div>

        <HeaderActions />
      </div>
    </header>
  )
}
