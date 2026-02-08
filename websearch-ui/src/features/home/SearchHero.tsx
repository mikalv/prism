import { useState, useRef, useEffect, KeyboardEvent } from 'react'
import { Search } from 'lucide-react'
import { Input } from '@/components/ui'

interface SearchHeroProps {
  onSearch: (query: string) => void
}

export function SearchHero({ onSearch }: SearchHeroProps) {
  const [query, setQuery] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  // Auto-focus on mount
  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  // Hotkey: "/" focuses input
  useEffect(() => {
    const handleKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key === '/' && document.activeElement !== inputRef.current) {
        e.preventDefault()
        inputRef.current?.focus()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [])

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && query.trim()) {
      onSearch(query.trim())
    }
  }

  return (
    <div className="flex flex-col items-center gap-6">
      <h1 className="text-4xl md:text-5xl font-bold text-[var(--text-primary)]">
        What do you want to know?
      </h1>
      <p className="text-lg text-[var(--text-secondary)]">
        Search the web and get AI-powered answers
      </p>
      <div className="w-full max-w-2xl">
        <Input
          ref={inputRef}
          size="lg"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Ask anything..."
          leftIcon={<Search className="w-5 h-5" />}
        />
      </div>
      <p className="text-sm text-[var(--text-muted)]">
        Press <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-tertiary)] font-mono">/</kbd> to
        focus
      </p>
    </div>
  )
}
