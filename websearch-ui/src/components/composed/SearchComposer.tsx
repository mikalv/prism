import { useRef, useEffect, KeyboardEvent } from 'react'
import { Plus, Search, Globe, ArrowUp } from 'lucide-react'

type ComposerVariant = 'hero' | 'inline' | 'sticky'
type SearchMode = 'search' | 'deepThink'

interface SearchComposerProps {
  variant: ComposerVariant
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  mode: SearchMode
  onModeChange: (mode: SearchMode) => void
  disabled?: boolean
  showQuickActions?: boolean
  autoFocus?: boolean
}

const wrapperStyles: Record<ComposerVariant, string> = {
  hero: `
    max-w-2xl w-full mx-auto
    rounded-2xl
    border border-[var(--border)]
    bg-[var(--bg-secondary)]
    shadow-[0px_4px_16px_0px_rgba(0,0,0,0.1)]
  `,
  inline: `
    max-w-3xl w-full
    rounded-xl
    border border-[var(--border)]
    bg-[var(--bg-secondary)]
    shadow-[0px_2px_8px_0px_rgba(0,0,0,0.08)]
  `,
  sticky: `
    flex-1 max-w-xl
    rounded-lg
    border border-[var(--border)]
    bg-[var(--bg-secondary)]
  `,
}

const textareaStyles: Record<ComposerVariant, string> = {
  hero: 'min-h-[72px] text-base p-4',
  inline: 'min-h-[56px] text-sm p-3',
  sticky: 'h-10 text-sm px-3 py-2',
}

export function SearchComposer({
  variant,
  value,
  onChange,
  onSubmit,
  mode,
  onModeChange,
  disabled = false,
  autoFocus = false,
}: SearchComposerProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return

    textarea.style.height = 'auto'
    const maxHeight = variant === 'sticky' ? 40 : 144
    textarea.style.height = `${Math.min(textarea.scrollHeight, maxHeight)}px`
  }, [value, variant])

  // Auto-focus
  useEffect(() => {
    if (autoFocus) {
      textareaRef.current?.focus()
    }
  }, [autoFocus])

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (value.trim() && !disabled) {
        onSubmit()
      }
    }
  }

  const isSticky = variant === 'sticky'

  return (
    <div className={wrapperStyles[variant]}>
      {/* Textarea area */}
      <div className={isSticky ? 'flex items-center' : ''}>
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Hva vil du vite?"
          disabled={disabled}
          rows={1}
          className={`
            w-full bg-transparent resize-none
            text-[var(--text-primary)]
            placeholder:text-[var(--text-muted)]
            focus:outline-none
            ${textareaStyles[variant]}
          `}
        />

        {/* Sticky: send button inline */}
        {isSticky && (
          <button
            onClick={onSubmit}
            disabled={!value.trim() || disabled}
            className={`
              p-2 mr-1 rounded-lg transition-colors shrink-0
              ${value.trim() && !disabled
                ? 'bg-[var(--accent)] text-white'
                : 'bg-[var(--bg-tertiary)] text-[var(--text-muted)]'}
            `}
          >
            <ArrowUp className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Bottom controls - not for sticky */}
      {!isSticky && (
        <div className="flex items-center justify-between px-3 pb-3 mt-1">
          {/* Left: attach + mode pills */}
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="p-1.5 rounded-lg hover:bg-[var(--bg-tertiary)] text-[var(--text-secondary)]"
            >
              <Plus className="w-5 h-5" />
            </button>

            <ModePill
              icon={<Search className="w-4 h-4" />}
              label="Search"
              active={mode === 'search'}
              onClick={() => onModeChange('search')}
            />

            <ModePill
              icon={<Globe className="w-4 h-4" />}
              label="Deep Think"
              active={mode === 'deepThink'}
              onClick={() => onModeChange('deepThink')}
            />
          </div>

          {/* Right: send */}
          <button
            onClick={onSubmit}
            disabled={!value.trim() || disabled}
            className={`
              p-2 rounded-lg transition-colors
              ${value.trim() && !disabled
                ? 'bg-[var(--accent)] text-white'
                : 'bg-[var(--bg-tertiary)] text-[var(--text-muted)]'}
            `}
          >
            <ArrowUp className="w-4 h-4" />
          </button>
        </div>
      )}
    </div>
  )
}

interface ModePillProps {
  icon: React.ReactNode
  label: string
  active: boolean
  onClick: () => void
}

function ModePill({ icon, label, active, onClick }: ModePillProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`
        flex items-center gap-1.5 px-2 py-1.5
        text-sm rounded-lg border transition-colors
        ${active
          ? 'bg-[var(--accent)]/10 border-[var(--accent)]/30 text-[var(--accent)]'
          : 'border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)]'}
      `}
    >
      {icon}
      <span className="hidden sm:inline">{label}</span>
    </button>
  )
}
