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

  const toggleAdvancedMode = () => setIsAdvancedMode(prev => !prev)

  return { isAdvancedMode, setAdvancedMode: setIsAdvancedMode, toggleAdvancedMode }
}
