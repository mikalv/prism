import { useState, KeyboardEvent } from 'react'
import { Search, Moon, Sun } from 'lucide-react'
import { Input, Button } from '@/components/ui'
import { useTheme } from '@/hooks'

interface TopBarProps {
  query: string
  onSearch: (query: string) => void
}

export function TopBar({ query: initialQuery, onSearch }: TopBarProps) {
  const [query, setQuery] = useState(initialQuery)
  const { mode, toggleMode } = useTheme()

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
            size="md"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Search..."
            leftIcon={<Search className="w-4 h-4" />}
          />
        </div>

        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={toggleMode}>
            {mode === 'dark' ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
          </Button>
        </div>
      </div>
    </header>
  )
}
