import { Moon, Sun, Settings, Lock, FolderKanban } from 'lucide-react'
import { Button } from '@/components/ui'
import { useTheme, useAdvancedMode } from '@/hooks'
import { useWindowManager } from '@/components/WindowManager'

export function HeaderActions() {
  const { mode, toggleMode } = useTheme()
  const { isAdvancedMode, toggleAdvancedMode } = useAdvancedMode()
  const { openWindow } = useWindowManager()

  // Placeholder Auth function
  const handleAuth = () => {
    alert('Auth integration not yet implemented.')
  }

  return (
    <div className="flex items-center gap-2">
      <Button
        variant="ghost"
        size="sm"
        onClick={() => openWindow('browser', 'Collection Browser')}
        title="Browse Collections"
      >
        <FolderKanban className="w-4 h-4 text-[var(--accent)]" />
      </Button>
      <Button variant="ghost" size="sm" onClick={handleAuth} title="Authenticate">
        <Lock className="w-4 h-4" />
      </Button>
      <Button 
        variant="ghost" 
        size="sm" 
        onClick={toggleAdvancedMode} 
        title="Toggle Advanced Mode"
        className={isAdvancedMode ? "text-[var(--accent)] bg-[var(--accent)]/10" : ""}
      >
        <Settings className="w-4 h-4" />
      </Button>
      <Button variant="ghost" size="sm" onClick={toggleMode} title="Toggle Theme">
        {mode === 'dark' ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
      </Button>
    </div>
  )
}
