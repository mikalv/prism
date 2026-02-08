import * as PopoverPrimitive from '@radix-ui/react-popover'
import { forwardRef, ComponentPropsWithoutRef, ElementRef } from 'react'

export const Popover = PopoverPrimitive.Root
export const PopoverTrigger = PopoverPrimitive.Trigger
export const PopoverAnchor = PopoverPrimitive.Anchor
export const PopoverPortal = PopoverPrimitive.Portal
export const PopoverArrow = PopoverPrimitive.Arrow

export const PopoverContent = forwardRef<
  ElementRef<typeof PopoverPrimitive.Content>,
  ComponentPropsWithoutRef<typeof PopoverPrimitive.Content>
>(({ className = '', sideOffset = 8, ...props }, ref) => (
  <PopoverPrimitive.Portal>
    <PopoverPrimitive.Content
      ref={ref}
      sideOffset={sideOffset}
      className={`
        w-80 p-4
        rounded-[var(--radius-lg)]
        bg-black/90 backdrop-blur-md
        border border-white/10
        shadow-[var(--shadow-xl)]
        text-white text-sm
        animate-fade-in-up
        z-50
        ${className}
      `}
      {...props}
    />
  </PopoverPrimitive.Portal>
))

PopoverContent.displayName = 'PopoverContent'
