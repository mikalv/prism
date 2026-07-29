import { useEffect, useRef, useState, ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { useWindowManager } from '../WindowManager'

interface WinBoxPortalProps {
  id: string
  title: string
  children: ReactNode
  onClose?: () => void
  width?: string | number
  height?: string | number
  x?: string | number
  y?: string | number
}

export function WinBoxPortal({ id, title, children, onClose, width = '80%', height = '80%', x = 'center', y = 'center' }: WinBoxPortalProps) {
  const [mountNode, setMountNode] = useState<HTMLElement | null>(null)
  const winboxRef = useRef<any>(null)
  const { windows, setMinimized, closeWindow } = useWindowManager()
  const myWindow = windows.find(w => w.id === id)

  // Sync state -> WinBox (for minimize from taskbar)
  useEffect(() => {
    if (winboxRef.current && myWindow) {
      if (myWindow.minimized) {
        winboxRef.current.minimize()
      } else {
        winboxRef.current.restore()
      }
    }
  }, [myWindow?.minimized])

  useEffect(() => {
    // Ensure WinBox is available globally
    if (typeof window === 'undefined' || !(window as any).WinBox) {
      console.error('WinBox is not available on window')
      return
    }

    const WinBox = (window as any).WinBox

    const wb = new WinBox({
      title,
      class: ['modern'],
      width,
      height,
      x,
      y,
      onclose: () => {
        if (onClose) onClose()
        closeWindow(id)
        return false // Let WinBox detach
      },
      onminimize: () => {
        setMinimized(id, true)
      },
      onrestore: () => {
        setMinimized(id, false)
      }
    })

    winboxRef.current = wb
    setMountNode(wb.body)

    return () => {
      if (winboxRef.current) {
        try {
          winboxRef.current.close()
        } catch (e) {
          // Ignore close errors on unmount
        }
      }
    }
  }, [title, width, height, x, y])

  if (!mountNode) return null

  return createPortal(children, mountNode)
}
