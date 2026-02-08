import type { AnswerModel } from '@/lib/types'
import { useStreamingText } from '@/hooks'
import { Card, Chip } from '@/components/ui'
import { CitationBadge } from './CitationBadge'
import { Sparkles, AlertCircle, MessageSquare } from 'lucide-react'

interface AnswerPanelProps {
  answer: AnswerModel
  variant?: 'full' | 'compact'
}

export function AnswerPanel({ answer, variant = 'full' }: AnswerPanelProps) {
  const { displayed: shortAnswer, isComplete } = useStreamingText(answer.shortAnswer, {
    delay: 200,
  })

  const displayKeyPoints = variant === 'compact' ? answer.keyPoints.slice(0, 3) : answer.keyPoints
  const displayFollowUps = variant === 'compact' ? answer.followUps.slice(0, 2) : answer.followUps

  return (
    <Card className="p-6">
      {/* Header */}
      <div className="flex items-center gap-2 mb-4">
        <Sparkles className="w-5 h-5 text-[var(--accent)]" />
        <h2 className="font-semibold text-[var(--text-primary)]">AI Answer</h2>
        <span
          className={`
            ml-auto px-2 py-0.5 rounded-full text-xs font-medium
            ${
              answer.confidence === 'high'
                ? 'bg-green-500/20 text-green-400'
                : answer.confidence === 'medium'
                  ? 'bg-yellow-500/20 text-yellow-400'
                  : 'bg-red-500/20 text-red-400'
            }
          `}
        >
          {answer.confidence} confidence
        </span>
      </div>

      {/* Short Answer */}
      <p className="text-lg text-[var(--text-primary)] mb-4 leading-relaxed">
        {shortAnswer}
        {!isComplete && <span className="animate-pulse">|</span>}
      </p>

      {/* Key Points */}
      {isComplete && (
        <div className="animate-fade-in-up">
          <h3 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">
            Key Points
          </h3>
          <ul className="space-y-3 mb-6">
            {displayKeyPoints.map((point, i) => (
              <li
                key={i}
                className="flex items-start gap-3 text-[var(--text-secondary)]"
                style={{ animationDelay: `${i * 100}ms` }}
              >
                <span className="w-1.5 h-1.5 rounded-full bg-[var(--accent)] mt-2 flex-shrink-0" />
                <span className="flex-1">{point}</span>
                {answer.citations[i] && (
                  <div className="flex gap-1">
                    {answer.citations[i].map((sourceId, j) => (
                      <CitationBadge key={j} number={parseInt(sourceId)} />
                    ))}
                  </div>
                )}
              </li>
            ))}
          </ul>

          {/* Concepts */}
          {variant === 'full' && answer.concepts.length > 0 && (
            <div className="mb-6">
              <h3 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">
                Related Concepts
              </h3>
              <div className="flex flex-wrap gap-2">
                {answer.concepts.map((concept) => (
                  <Chip key={concept}>{concept}</Chip>
                ))}
              </div>
            </div>
          )}

          {/* Caveats */}
          {variant === 'full' && answer.caveats.length > 0 && (
            <div className="mb-6 p-3 rounded-[var(--radius-md)] bg-yellow-500/10 border border-yellow-500/20">
              <div className="flex items-center gap-2 mb-2">
                <AlertCircle className="w-4 h-4 text-yellow-400" />
                <h3 className="text-sm font-semibold text-yellow-400">Important Notes</h3>
              </div>
              <ul className="space-y-1">
                {answer.caveats.map((caveat, i) => (
                  <li key={i} className="text-sm text-yellow-200/80">
                    {caveat}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Follow-ups */}
          {displayFollowUps.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2 flex items-center gap-2">
                <MessageSquare className="w-4 h-4" />
                Follow-up Questions
              </h3>
              <div className="flex flex-col gap-2">
                {displayFollowUps.map((question, i) => (
                  <button
                    key={i}
                    className="
                      text-left px-3 py-2
                      rounded-[var(--radius-md)]
                      bg-[var(--bg-tertiary)]
                      text-[var(--text-secondary)]
                      hover:bg-[var(--border)]
                      transition-colors
                      text-sm
                    "
                  >
                    {question}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </Card>
  )
}
