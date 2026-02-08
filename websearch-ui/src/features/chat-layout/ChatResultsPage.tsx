import { SearchComposer, ThemePicker } from '@/components/composed'
import { useScrollComposer } from '@/hooks'
import { ResultsLayout } from '@/features/search/ResultsLayout'
import { ResultsList } from '@/features/search/ResultsList'
import { DiscussionsList } from '@/features/search/DiscussionsList'
import { AnswerPanel } from '@/features/search/AnswerPanel'
import type { SearchResult, AnswerModel, Intent } from '@/lib/types'

type SearchMode = 'search' | 'deepThink'

interface ChatResultsPageProps {
  query: string
  effectiveIntent: Intent
  results: SearchResult[] | null
  discussions: SearchResult[] | null
  answer: AnswerModel | null
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  searchMode: SearchMode
  onModeChange: (mode: SearchMode) => void
}

export function ChatResultsPage({
  query,
  effectiveIntent,
  results,
  discussions,
  answer,
  value,
  onChange,
  onSubmit,
  searchMode,
  onModeChange,
}: ChatResultsPageProps) {
  const { composerRef, showSticky } = useScrollComposer()

  const serpVariant = effectiveIntent === 'search' ? 'full' : 'compact'
  const answerVariant = effectiveIntent === 'chat' ? 'full' : 'compact'

  return (
    <div className="min-h-screen flex flex-col">
      {/* Header with optional sticky composer */}
      <header className="sticky top-0 z-20 bg-[var(--bg-primary)] border-b border-[var(--border)]">
        <div className="max-w-6xl mx-auto px-4 h-14 flex items-center gap-4">
          <button
            onClick={() => window.location.reload()}
            className="text-xl font-semibold text-[var(--accent)] hover:opacity-80 transition-opacity shrink-0"
          >
            WebSearch
          </button>

          {/* Sticky composer - only visible when scrolled */}
          {showSticky && (
            <SearchComposer
              variant="sticky"
              value={value}
              onChange={onChange}
              onSubmit={onSubmit}
              mode={searchMode}
              onModeChange={onModeChange}
            />
          )}

          {/* Spacer to push theme picker to the right */}
          <div className="flex-1" />

          {/* Theme picker */}
          <ThemePicker />
        </div>
      </header>

      {/* Content */}
      <main className="flex-1 px-4 py-6">
        <div className="max-w-6xl mx-auto">
          {/* Inline composer - measured for scroll trigger */}
          <div ref={composerRef} className="mb-6">
            <SearchComposer
              variant="inline"
              value={value}
              onChange={onChange}
              onSubmit={onSubmit}
              mode={searchMode}
              onModeChange={onModeChange}
            />
          </div>

          {/* Results */}
          <ResultsLayout
            intent={effectiveIntent}
            answerPanel={
              answer && <AnswerPanel answer={answer} variant={answerVariant} />
            }
            serpPanel={
              <div>
                {results && <ResultsList results={results} variant={serpVariant} />}
                {discussions && serpVariant === 'full' && (
                  <DiscussionsList discussions={discussions} />
                )}
              </div>
            }
          />
        </div>
      </main>
    </div>
  )
}
