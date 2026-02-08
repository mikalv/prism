import { Palette, Check } from 'lucide-react'
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui'
import { useTheme } from '@/hooks'
import type { ThemeName, ThemeMode } from '@/lib/themes'

interface ThemeOption {
  theme: ThemeName
  mode: ThemeMode
  label: string
  colors: {
    bg: string
    accent: string
  }
}

const THEME_OPTIONS: ThemeOption[] = [
  // Teal (Perplexity)
  { theme: 'teal', mode: 'dark', label: 'Teal Dark', colors: { bg: '#0f0f0f', accent: '#20b2aa' } },
  { theme: 'teal', mode: 'light', label: 'Teal Light', colors: { bg: '#ffffff', accent: '#20b2aa' } },
  // Indigo (klassisk tech)
  { theme: 'indigo', mode: 'dark', label: 'Indigo Dark', colors: { bg: '#0c0a1d', accent: '#6366f1' } },
  { theme: 'indigo', mode: 'light', label: 'Indigo Light', colors: { bg: '#fafafa', accent: '#6366f1' } },
  // Rose (soft modern)
  { theme: 'rose', mode: 'dark', label: 'Rose Dark', colors: { bg: '#0f0506', accent: '#f43f5e' } },
  { theme: 'rose', mode: 'light', label: 'Rose Light', colors: { bg: '#fff1f2', accent: '#f43f5e' } },
  // Amber (warm)
  { theme: 'amber', mode: 'dark', label: 'Amber Dark', colors: { bg: '#0f0a00', accent: '#f59e0b' } },
  { theme: 'amber', mode: 'light', label: 'Amber Light', colors: { bg: '#fffbeb', accent: '#f59e0b' } },
  // Emerald (nature)
  { theme: 'emerald', mode: 'dark', label: 'Emerald Dark', colors: { bg: '#021f13', accent: '#10b981' } },
  { theme: 'emerald', mode: 'light', label: 'Emerald Light', colors: { bg: '#ecfdf5', accent: '#10b981' } },
  // Neutral (z.ai)
  { theme: 'neutral', mode: 'dark', label: 'Neutral Dark', colors: { bg: '#141618', accent: '#6b7280' } },
  { theme: 'neutral', mode: 'light', label: 'Neutral Light', colors: { bg: '#f4f6f8', accent: '#6b7280' } },
]

export function ThemePicker() {
  const { theme, mode, setTheme, setMode } = useTheme()

  const handleSelect = (option: ThemeOption) => {
    setTheme(option.theme)
    setMode(option.mode)
  }

  const isSelected = (option: ThemeOption) =>
    option.theme === theme && option.mode === mode

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          className="p-2 rounded-lg hover:bg-[var(--bg-tertiary)] text-[var(--text-secondary)] transition-colors"
          aria-label="Velg tema"
        >
          <Palette className="w-5 h-5" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-52 p-2">
        <p className="text-xs text-white/60 mb-2 px-2">Velg tema</p>
        <div className="space-y-1 max-h-80 overflow-y-auto">
          {THEME_OPTIONS.map((option) => (
            <button
              key={`${option.theme}-${option.mode}`}
              onClick={() => handleSelect(option)}
              className={`
                w-full flex items-center gap-3 px-2 py-2 rounded-lg
                transition-colors text-left
                ${isSelected(option)
                  ? 'bg-white/20'
                  : 'hover:bg-white/10'}
              `}
            >
              {/* Color swatch */}
              <div
                className="w-6 h-6 rounded-md border border-white/20 flex items-center justify-center"
                style={{ backgroundColor: option.colors.bg }}
              >
                <div
                  className="w-3 h-3 rounded-full"
                  style={{ backgroundColor: option.colors.accent }}
                />
              </div>

              {/* Label */}
              <span className="flex-1 text-sm">{option.label}</span>

              {/* Check mark */}
              {isSelected(option) && (
                <Check className="w-4 h-4 text-[#20b2aa]" />
              )}
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  )
}
