import { useState, useEffect, useCallback } from 'react'
import { ThemeState, ThemeName, ThemeMode, THEME_STORAGE_KEY, DEFAULT_THEME } from '@/lib/themes'

function getStoredTheme(): ThemeState {
  if (typeof window === 'undefined') return DEFAULT_THEME
  const stored = localStorage.getItem(THEME_STORAGE_KEY)
  if (!stored) return DEFAULT_THEME
  try {
    return JSON.parse(stored) as ThemeState
  } catch {
    return DEFAULT_THEME
  }
}

export function useTheme() {
  const [state, setState] = useState<ThemeState>(getStoredTheme)

  useEffect(() => {
    const html = document.documentElement
    html.className = `theme-${state.theme} ${state.mode}`
    localStorage.setItem(THEME_STORAGE_KEY, JSON.stringify(state))
  }, [state])

  const setTheme = useCallback((theme: ThemeName) => {
    setState((s) => ({ ...s, theme }))
  }, [])

  const setMode = useCallback((mode: ThemeMode) => {
    setState((s) => ({ ...s, mode }))
  }, [])

  const toggleMode = useCallback(() => {
    setState((s) => ({
      ...s,
      mode: s.mode === 'light' ? 'dark' : 'light',
    }))
  }, [])

  return {
    theme: state.theme,
    mode: state.mode,
    setTheme,
    setMode,
    toggleMode,
  }
}
