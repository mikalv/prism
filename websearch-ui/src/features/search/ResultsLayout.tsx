import type { ReactNode } from 'react'
import type { Intent } from '@/lib/types'

interface ResultsLayoutProps {
  intent: Intent
  answerPanel: ReactNode
  serpPanel: ReactNode
}

export function ResultsLayout({ intent, answerPanel, serpPanel }: ResultsLayoutProps) {
  const isChat = intent === 'chat'

  return (
    <div
      className={`
        grid gap-6 transition-all duration-300
        ${isChat ? 'lg:grid-cols-[65fr_35fr]' : 'lg:grid-cols-[60fr_40fr]'}
        grid-cols-1
      `}
    >
      {isChat ? (
        <>
          {answerPanel}
          {serpPanel}
        </>
      ) : (
        <>
          {serpPanel}
          {answerPanel}
        </>
      )}
    </div>
  )
}
