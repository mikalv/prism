import { InputHTMLAttributes, forwardRef, ReactNode } from 'react'

type InputSize = 'sm' | 'md' | 'lg'

interface InputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'size'> {
  size?: InputSize
  leftIcon?: ReactNode
  rightIcon?: ReactNode
}

const sizeStyles: Record<InputSize, string> = {
  sm: 'h-9 text-sm px-3',
  md: 'h-11 text-base px-4',
  lg: 'h-14 text-lg px-5',
}

const iconPadding: Record<InputSize, { left: string; right: string }> = {
  sm: { left: 'pl-9', right: 'pr-9' },
  md: { left: 'pl-11', right: 'pr-11' },
  lg: { left: 'pl-14', right: 'pr-14' },
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ size = 'md', leftIcon, rightIcon, className = '', ...props }, ref) => {
    return (
      <div className="relative w-full">
        {leftIcon && (
          <div className="absolute left-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)]">
            {leftIcon}
          </div>
        )}
        <input
          ref={ref}
          className={`
            w-full
            rounded-[var(--radius-lg)]
            bg-[var(--bg-secondary)]
            border border-[var(--border)]
            text-[var(--text-primary)]
            placeholder:text-[var(--text-muted)]
            transition-colors duration-150
            focus:outline-none focus:border-[var(--accent)] focus:ring-1 focus:ring-[var(--accent)]
            ${sizeStyles[size]}
            ${leftIcon ? iconPadding[size].left : ''}
            ${rightIcon ? iconPadding[size].right : ''}
            ${className}
          `}
          {...props}
        />
        {rightIcon && (
          <div className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)]">
            {rightIcon}
          </div>
        )}
      </div>
    )
  }
)

Input.displayName = 'Input'
