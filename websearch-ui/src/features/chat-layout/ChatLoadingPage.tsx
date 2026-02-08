import type { LoadingPhase } from '@/lib/types'
import { MinimalHeader } from './MinimalHeader'
import { LoadingSpinner, SkeletonResults } from '@/features/loading'

interface ChatLoadingPageProps {
  query: string
  phase: LoadingPhase | null
}

const PHASE_LABELS: Record<LoadingPhase, string> = {
  understanding: 'Forstår spørsmålet...',
  searching: 'Søker i kilder...',
  synthesizing: 'Lager svar...',
}

export function ChatLoadingPage({ query, phase }: ChatLoadingPageProps) {
  return (
    <div className="min-h-screen flex flex-col">
      <MinimalHeader />

      {/* Query display */}
      <div className="px-4 py-4 border-b border-[var(--border)]">
        <div className="max-w-3xl mx-auto">
          <div className="px-4 py-3 rounded-xl bg-[var(--bg-secondary)] border border-[var(--border)]">
            <p className="text-[var(--text-primary)]">{query}</p>
          </div>
        </div>
      </div>

      {/* Loading content */}
      <main className="flex-1 flex flex-col items-center justify-center gap-6 p-8">
        <LoadingSpinner />
        <p className="text-lg text-[var(--text-secondary)]">
          {phase ? PHASE_LABELS[phase] : 'Laster...'}
        </p>
        <SkeletonResults />
      </main>
    </div>
  )
}
