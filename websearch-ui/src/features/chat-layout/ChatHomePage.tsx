import { SearchComposer } from '@/components/composed/SearchComposer'
import { MinimalHeader } from './MinimalHeader'
import { Chip } from '@/components/ui'
import { Sparkles, Code, FileText } from 'lucide-react'

type SearchMode = 'search' | 'deepThink'

interface ChatHomePageProps {
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  mode: SearchMode
  onModeChange: (mode: SearchMode) => void
}

const QUICK_ACTIONS = [
  { label: 'AI Overview', icon: Sparkles, query: 'What is artificial intelligence?' },
  { label: 'Write Code', icon: Code, query: 'How do I write a React hook?' },
  { label: 'Summarize', icon: FileText, query: 'Summarize the latest tech news' },
]

export function ChatHomePage({
  value,
  onChange,
  onSubmit,
  mode,
  onModeChange,
}: ChatHomePageProps) {
  const handleQuickAction = (query: string) => {
    onChange(query)
    // Small delay to show the query, then submit
    setTimeout(() => onSubmit(), 100)
  }

  return (
    <div className="min-h-screen flex flex-col">
      <MinimalHeader />

      <main className="flex-1 flex flex-col items-center justify-center px-4 pb-32">
        <h1 className="text-3xl md:text-4xl font-semibold text-[var(--text-primary)] mb-8 text-center">
          Hva vil du vite?
        </h1>

        <SearchComposer
          variant="hero"
          value={value}
          onChange={onChange}
          onSubmit={onSubmit}
          mode={mode}
          onModeChange={onModeChange}
          autoFocus
        />

        {/* Quick actions */}
        <div className="flex flex-wrap justify-center gap-2 mt-6">
          {QUICK_ACTIONS.map(({ label, icon: Icon, query }) => (
            <Chip key={label} onClick={() => handleQuickAction(query)}>
              <Icon className="w-4 h-4 mr-1.5" />
              {label}
            </Chip>
          ))}
        </div>

        <p className="text-sm text-[var(--text-muted)] mt-8">
          Trykk <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-tertiary)] font-mono text-xs">Enter</kbd> for å søke
        </p>
      </main>
    </div>
  )
}
