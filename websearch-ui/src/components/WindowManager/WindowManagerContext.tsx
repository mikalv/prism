import { createContext, useContext, useState, ReactNode, useCallback } from 'react'

export interface WindowState {
  id: string
  title: string
  type: string
  minimized: boolean
}

interface WindowManagerContextType {
  windows: WindowState[]
  openWindow: (type: string, title: string) => void
  closeWindow: (id: string) => void
  setMinimized: (id: string, minimized: boolean) => void
  toggleMinimize: (id: string) => void
}

const WindowManagerContext = createContext<WindowManagerContextType | null>(null)

let windowIdCounter = 0

export function WindowManagerProvider({ children }: { children: ReactNode }) {
  const [windows, setWindows] = useState<WindowState[]>([])

  const openWindow = useCallback((type: string, title: string) => {
    const id = `win-${Date.now()}-${windowIdCounter++}`
    setWindows(prev => [...prev, { id, title, type, minimized: false }])
  }, [])

  const closeWindow = useCallback((id: string) => {
    setWindows(prev => prev.filter(w => w.id !== id))
  }, [])

  const setMinimized = useCallback((id: string, minimized: boolean) => {
    setWindows(prev => prev.map(w => w.id === id ? { ...w, minimized } : w))
  }, [])

  const toggleMinimize = useCallback((id: string) => {
    setWindows(prev => prev.map(w => w.id === id ? { ...w, minimized: !w.minimized } : w))
  }, [])

  return (
    <WindowManagerContext.Provider value={{
      windows,
      openWindow,
      closeWindow,
      setMinimized,
      toggleMinimize
    }}>
      {children}
    </WindowManagerContext.Provider>
  )
}

export function useWindowManager() {
  const ctx = useContext(WindowManagerContext)
  if (!ctx) throw new Error('useWindowManager must be used within WindowManagerProvider')
  return ctx
}
