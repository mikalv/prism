import type { LoadingPhase } from '@/lib/types'
import { LoadingSpinner } from './LoadingSpinner'
import { SkeletonResults } from './SkeletonResults'

interface LoadingPageProps {
  query: string
  phase: LoadingPhase | null
}

const PHASE_LABELS: Record<LoadingPhase, string> = {
  understanding: 'Understanding query...',
  searching: 'Searching sources...',
  synthesizing: 'Synthesizing answer...',
}

export function LoadingPage({ query, phase }: LoadingPageProps) {
  return (
    <div className="min-h-screen flex flex-col">
      {/* TopBar placeholder */}
      <header className="sticky top-0 z-10 px-6 py-4 bg-[var(--bg-primary)] border-b border-[var(--border)]">
        <div className="max-w-6xl mx-auto flex items-center gap-4">
          <span className="text-xl font-semibold text-[var(--accent)]">WebSearch</span>
          <div className="flex-1 max-w-xl">
            <div className="h-11 px-4 rounded-[var(--radius-lg)] bg-[var(--bg-secondary)] border border-[var(--border)] flex items-center">
              <span className="text-[var(--text-muted)]">{query}</span>
            </div>
          </div>
        </div>
      </header>

      {/* Loading content */}
      <main className="flex-1 flex flex-col items-center justify-center gap-6 p-8">
        <LoadingSpinner />
        <p className="text-lg text-[var(--text-secondary)]">
          {phase ? PHASE_LABELS[phase] : 'Loading...'}
        </p>
        <SkeletonResults />
      </main>
    </div>
  )
}
