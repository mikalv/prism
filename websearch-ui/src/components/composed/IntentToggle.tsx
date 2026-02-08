import type { Intent } from '@/lib/types'
import { Sparkles, Search } from 'lucide-react'

interface IntentToggleProps {
  intent: Intent
  onChange: (intent: Intent) => void
}

export function IntentToggle({ intent, onChange }: IntentToggleProps) {
  return (
    <div className="inline-flex rounded-full bg-[var(--bg-tertiary)] p-1">
      <button
        onClick={() => onChange('chat')}
        className={`
          flex items-center gap-1.5 px-3 py-1.5 rounded-full text-sm font-medium transition-colors
          ${
            intent === 'chat'
              ? 'bg-[var(--accent)] text-white'
              : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
          }
        `}
      >
        <Sparkles className="w-4 h-4" />
        Answer
      </button>
      <button
        onClick={() => onChange('search')}
        className={`
          flex items-center gap-1.5 px-3 py-1.5 rounded-full text-sm font-medium transition-colors
          ${
            intent === 'search'
              ? 'bg-[var(--accent)] text-white'
              : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
          }
        `}
      >
        <Search className="w-4 h-4" />
        Search
      </button>
    </div>
  )
}
