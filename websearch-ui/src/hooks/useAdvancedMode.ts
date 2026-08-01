import { useState, useEffect } from 'react'

const STORAGE_KEY = 'prism_advanced_mode'

export function useAdvancedMode() {
  const [isAdvancedMode, setIsAdvancedMode] = useState(() => {
    if (typeof window !== 'undefined') {
      return localStorage.getItem(STORAGE_KEY) === 'true'
    }
    return false
  })

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, String(isAdvancedMode))
  }, [isAdvancedMode])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Toggle on Ctrl+Shift+A, Cmd+Shift+A, or Alt/Option+Shift+A
      if ((e.ctrlKey || e.metaKey || e.altKey) && e.shiftKey && e.key.toLowerCase() === 'a') {
        e.preventDefault()
        setIsAdvancedMode(prev => !prev)
      }
    }
    
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])

  const toggleAdvancedMode = () => setIsAdvancedMode(prev => !prev)

  return { isAdvancedMode, setAdvancedMode: setIsAdvancedMode, toggleAdvancedMode }
}
