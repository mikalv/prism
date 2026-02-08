import { SearchHero } from './SearchHero'
import { QuickActions } from './QuickActions'

interface HomePageProps {
  onSearch: (query: string) => void
}

export function HomePage({ onSearch }: HomePageProps) {
  return (
    <div className="min-h-screen flex flex-col items-center justify-center p-8">
      <div className="w-full max-w-3xl flex flex-col items-center gap-8">
        <SearchHero onSearch={onSearch} />
        <QuickActions onAction={onSearch} />
      </div>
    </div>
  )
}
