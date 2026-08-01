import { SearchHero } from './SearchHero'
import { QuickActions } from './QuickActions'
import { HeaderActions } from '@/components/composed'

interface HomePageProps {
  onSearch: (query: string, collections?: string[]) => void
}

export function HomePage({ onSearch }: HomePageProps) {
  return (
    <div className="min-h-screen flex flex-col items-center justify-center p-8 relative">
      <div className="absolute top-6 right-6">
        <HeaderActions />
      </div>
      <div className="w-full max-w-3xl flex flex-col items-center gap-8">
        <SearchHero onSearch={onSearch} />
        <QuickActions onAction={(query) => onSearch(query, [])} />
      </div>
    </div>
  )
}

