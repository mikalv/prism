import { Button } from '@/components/ui/button'
import { RefreshCw } from 'lucide-react'

interface PageHeaderProps {
  title: string
  onRefresh?: () => void
  isRefreshing?: boolean
}

export function PageHeader({ title, onRefresh, isRefreshing }: PageHeaderProps) {
  return (
    <div className="flex items-center justify-between">
      <h2 className="text-2xl font-bold">{title}</h2>
      {onRefresh && (
        <Button
          variant="outline"
          size="sm"
          onClick={onRefresh}
          disabled={isRefreshing}
        >
          <RefreshCw className={`mr-2 h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
          Refresh
        </Button>
      )}
    </div>
  )
}
