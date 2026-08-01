import { Settings2 } from 'lucide-react'
import { useAdvancedMode } from '@/hooks'
import { Button } from '@/components/ui'

export function FloatingAdvancedToggle() {
  const { isAdvancedMode, toggleAdvancedMode } = useAdvancedMode()

  // Only show the floating button if Advanced Mode is NOT active. 
  // If it is active, the Taskbar is visible anyway, which serves as an indicator and can have the toggle in the future if needed,
  // or we can just keep it always visible for ease of toggling.
  // Actually, we'll keep it always visible but very subtle.
  
  return (
    <div className={`fixed z-[9000] transition-all duration-300 ${isAdvancedMode ? 'bottom-12 right-4' : 'bottom-4 right-4'}`}>
      <Button
        variant="ghost"
        size="sm"
        onClick={toggleAdvancedMode}
        title="Toggle Advanced Mode (Ctrl+Shift+A)"
        className={`rounded-full p-2 shadow-lg backdrop-blur border ${
          isAdvancedMode 
            ? "bg-[var(--accent)]/20 text-[var(--accent)] border-[var(--accent)]/30 hover:bg-[var(--accent)]/30" 
            : "bg-[var(--bg-secondary)]/50 text-[var(--text-secondary)] border-[var(--border)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
        }`}
      >
        <Settings2 className="w-4 h-4" />
      </Button>
    </div>
  )
}
