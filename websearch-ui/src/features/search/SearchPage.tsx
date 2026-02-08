import type { SearchResult, AnswerModel, Intent } from '@/lib/types'
import { TopBar } from './TopBar'
import { ResultsLayout } from './ResultsLayout'
import { ResultsList } from './ResultsList'
import { DiscussionsList } from './DiscussionsList'
import { AnswerPanel } from './AnswerPanel'
import { IntentToggle } from '@/components/composed/IntentToggle'

interface SearchPageProps {
  query: string
  effectiveIntent: Intent
  results: SearchResult[] | null
  discussions: SearchResult[] | null
  answer: AnswerModel | null
  onNewSearch: (query: string) => void
  setIntentOverride: (intent: Intent | null) => void
}

export function SearchPage({
  query,
  effectiveIntent,
  results,
  discussions,
  answer,
  onNewSearch,
  setIntentOverride,
}: SearchPageProps) {
  const serpVariant = effectiveIntent === 'search' ? 'full' : 'compact'
  const answerVariant = effectiveIntent === 'chat' ? 'full' : 'compact'

  return (
    <div className="min-h-screen flex flex-col">
      <TopBar query={query} onSearch={onNewSearch} />

      <main className="flex-1 px-6 py-6">
        <div className="max-w-6xl mx-auto">
          {/* Intent Toggle */}
          <div className="mb-6 flex justify-center">
            <IntentToggle
              intent={effectiveIntent}
              onChange={(intent) => setIntentOverride(intent)}
            />
          </div>

          {/* Results Layout */}
          <ResultsLayout
            intent={effectiveIntent}
            answerPanel={answer && <AnswerPanel answer={answer} variant={answerVariant} />}
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
