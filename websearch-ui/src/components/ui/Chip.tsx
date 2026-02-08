import { ButtonHTMLAttributes, forwardRef } from 'react'

interface ChipProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean
}

export const Chip = forwardRef<HTMLButtonElement, ChipProps>(
  ({ active = false, className = '', children, ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={`
          inline-flex items-center
          px-3 py-1.5
          rounded-full
          text-sm font-medium
          transition-colors duration-150
          ${
            active
              ? 'bg-[var(--accent)] text-white'
              : 'bg-[var(--bg-tertiary)] text-[var(--text-secondary)] hover:bg-[var(--border)]'
          }
          ${className}
        `}
        {...props}
      >
        {children}
      </button>
    )
  }
)

Chip.displayName = 'Chip'
