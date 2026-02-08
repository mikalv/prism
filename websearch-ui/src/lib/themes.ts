export type ThemeName = 'neutral' | 'teal' | 'indigo' | 'rose' | 'amber' | 'emerald'
export type ThemeMode = 'light' | 'dark'

export interface ThemeState {
  theme: ThemeName
  mode: ThemeMode
}

export const THEME_STORAGE_KEY = 'websearch-theme'

export const DEFAULT_THEME: ThemeState = {
  theme: 'teal',
  mode: 'dark',
}
