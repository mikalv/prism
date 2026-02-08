import type { SearchResult } from '@/lib/types'
import { Card } from '@/components/ui'
import { ExternalLink } from 'lucide-react'

interface ResultCardProps {
  result: SearchResult
  compact?: boolean
}

export function ResultCard({ result, compact = false }: ResultCardProps) {
  return (
    <Card hover className="p-4">
      <a
        href={result.url}
        target="_blank"
        rel="noopener noreferrer"
        className="block group"
      >
        <div className="flex items-start gap-3">
          {result.faviconUrl && (
            <img
              src={result.faviconUrl}
              alt=""
              className="w-4 h-4 mt-1 rounded"
              onError={(e) => {
                e.currentTarget.style.display = 'none'
              }}
            />
          )}
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <span className="text-xs text-[var(--text-muted)] truncate">
                {result.displayDomain}
              </span>
              <ExternalLink className="w-3 h-3 text-[var(--text-muted)] opacity-0 group-hover:opacity-100 transition-opacity" />
            </div>
            <h3 className="font-medium text-[var(--accent)] group-hover:underline line-clamp-2">
              {result.title}
            </h3>
            {!compact && (
              <p className="mt-1 text-sm text-[var(--text-secondary)] line-clamp-2">
                {result.snippet}
              </p>
            )}
            {result.publishedAt && !compact && (
              <span className="mt-2 text-xs text-[var(--text-muted)]">
                {result.publishedAt}
              </span>
            )}
          </div>
        </div>
      </a>
    </Card>
  )
}
