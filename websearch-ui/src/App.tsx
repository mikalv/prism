import { useSearch, useTheme } from '@/hooks'
import { ClassicLayout } from '@/features/classic-layout'

export default function App() {
  useTheme()
  const search = useSearch()

  return <ClassicLayout search={search} />
}
