import { useState, useRef, useEffect, KeyboardEvent } from 'react'
import { Search, ChevronDown, Database } from 'lucide-react'
import { Input } from '@/components/ui'
import { getCollections } from '@/lib/api'

interface SearchHeroProps {
  onSearch: (query: string, collection?: string) => void
}

export function SearchHero({ onSearch }: SearchHeroProps) {
  const [query, setQuery] = useState('')
  const [collections, setCollections] = useState<string[]>([])
  const [selectedCollection, setSelectedCollection] = useState<string>('')
  const [dropdownOpen, setDropdownOpen] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const dropdownRef = useRef<HTMLDivElement>(null)

  // Load collections on mount
  useEffect(() => {
    getCollections().then((cols) => {
      setCollections(cols)
      // Don't pre-select - "All collections" is default
    })
  }, [])

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

  // Close dropdown on outside click
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setDropdownOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && query.trim()) {
      onSearch(query.trim(), selectedCollection || undefined)
    }
  }

  const handleCollectionSelect = (collection: string) => {
    setSelectedCollection(collection)
    setDropdownOpen(false)
    inputRef.current?.focus()
  }

  return (
    <div className="flex flex-col items-center gap-6">
      <h1 className="text-4xl md:text-5xl font-bold text-[var(--text-primary)]">
        Prism Search
      </h1>
      <p className="text-lg text-[var(--text-secondary)]">
        Fast hybrid search across your collections
      </p>

      {/* Search bar with collection selector */}
      <div className="w-full max-w-3xl">
        <div className="flex gap-2">
          {/* Collection dropdown */}
          {collections.length > 0 && (
            <div ref={dropdownRef} className="relative">
              <button
                onClick={() => setDropdownOpen(!dropdownOpen)}
                className="
                  h-14 px-4
                  flex items-center gap-2
                  rounded-[var(--radius-lg)]
                  bg-[var(--bg-secondary)]
                  border border-[var(--border)]
                  text-[var(--text-secondary)]
                  hover:border-[var(--accent)]
                  transition-colors duration-150
                  whitespace-nowrap
                "
              >
                <Database className="w-4 h-4" />
                <span className="text-sm">
                  {selectedCollection || 'All'}
                </span>
                <ChevronDown className={`w-4 h-4 transition-transform ${dropdownOpen ? 'rotate-180' : ''}`} />
              </button>

              {dropdownOpen && (
                <div className="
                  absolute top-full left-0 mt-1 z-50
                  min-w-[160px]
                  rounded-[var(--radius-lg)]
                  bg-[var(--bg-secondary)]
                  border border-[var(--border)]
                  shadow-lg
                  overflow-hidden
                ">
                  <button
                    onClick={() => handleCollectionSelect('')}
                    className={`
                      w-full px-4 py-2 text-left text-sm
                      hover:bg-[var(--bg-tertiary)]
                      ${!selectedCollection ? 'text-[var(--accent)]' : 'text-[var(--text-primary)]'}
                    `}
                  >
                    All collections
                  </button>
                  {collections.map((col) => (
                    <button
                      key={col}
                      onClick={() => handleCollectionSelect(col)}
                      className={`
                        w-full px-4 py-2 text-left text-sm
                        hover:bg-[var(--bg-tertiary)]
                        ${selectedCollection === col ? 'text-[var(--accent)]' : 'text-[var(--text-primary)]'}
                      `}
                    >
                      {col}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Search input */}
          <div className="flex-1">
            <Input
              ref={inputRef}
              size="lg"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Search..."
              leftIcon={<Search className="w-5 h-5" />}
            />
          </div>
        </div>
      </div>

      <p className="text-sm text-[var(--text-muted)]">
        Press <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-tertiary)] font-mono">/</kbd> to focus
      </p>
    </div>
  )
}
