import { useSearch, useTheme } from '@/hooks'
import { ClassicLayout } from '@/features/classic-layout'
import { ChatLayout } from '@/features/chat-layout'

// Layout mode - change this to switch between layouts
const LAYOUT_MODE: 'classic' | 'chat' = 'chat'

export default function App() {
  useTheme()
  const search = useSearch()

  if (LAYOUT_MODE === 'classic') {
    return <ClassicLayout search={search} />
  }

  return <ChatLayout search={search} />
}
