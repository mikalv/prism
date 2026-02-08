import * as SwitchPrimitive from '@radix-ui/react-switch'
import { forwardRef, ComponentPropsWithoutRef, ElementRef } from 'react'

export const Switch = forwardRef<
  ElementRef<typeof SwitchPrimitive.Root>,
  ComponentPropsWithoutRef<typeof SwitchPrimitive.Root>
>(({ className = '', ...props }, ref) => (
  <SwitchPrimitive.Root
    ref={ref}
    className={`
      w-10 h-6
      rounded-full
      bg-[var(--bg-tertiary)]
      data-[state=checked]:bg-[var(--accent)]
      transition-colors duration-150
      focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2
      ${className}
    `}
    {...props}
  >
    <SwitchPrimitive.Thumb
      className={`
        block w-4 h-4
        rounded-full
        bg-white
        translate-x-1
        transition-transform duration-150
        data-[state=checked]:translate-x-5
      `}
    />
  </SwitchPrimitive.Root>
))

Switch.displayName = 'Switch'
