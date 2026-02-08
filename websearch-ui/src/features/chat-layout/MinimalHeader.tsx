import { ThemePicker } from '@/components/composed'

export function MinimalHeader() {
  return (
    <header className="px-4 h-14 flex items-center justify-between">
      <button
        onClick={() => window.location.reload()}
        className="text-xl font-semibold text-[var(--accent)] hover:opacity-80 transition-opacity"
      >
        WebSearch
      </button>

      <ThemePicker />
    </header>
  )
}
