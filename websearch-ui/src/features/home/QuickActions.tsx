import { Chip } from '@/components/ui'
import { Sparkles, Code, FileText, Image } from 'lucide-react'

interface QuickActionsProps {
  onAction: (query: string) => void
}

const QUICK_ACTIONS = [
  { label: 'AI Overview', icon: Sparkles, query: 'What is artificial intelligence?' },
  { label: 'Write Code', icon: Code, query: 'How do I write a React hook?' },
  { label: 'Summarize', icon: FileText, query: 'Summarize the latest tech news' },
  { label: 'Create Image', icon: Image, query: 'How do I create images with AI?' },
]

export function QuickActions({ onAction }: QuickActionsProps) {
  return (
    <div className="flex flex-wrap justify-center gap-2">
      {QUICK_ACTIONS.map(({ label, icon: Icon, query }) => (
        <Chip key={label} onClick={() => onAction(query)}>
          <Icon className="w-4 h-4 mr-1.5" />
          {label}
        </Chip>
      ))}
    </div>
  )
}
