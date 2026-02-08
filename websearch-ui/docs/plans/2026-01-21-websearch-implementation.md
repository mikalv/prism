# WebSearch Frontend Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a hybrid search/chat UI inspired by Perplexity and z.ai with adaptive layout, theme system, and streaming effects.

**Architecture:** SPA with React state machine managing home→loading→results transitions. Intent classification determines layout weighting. Mock data simulates backend responses.

**Tech Stack:** React 18, Vite, TypeScript, Tailwind CSS v4, Radix UI, Lucide React

---

## Task 1: Project Setup

**Files:**
- Create: `websearch/package.json`
- Create: `websearch/tsconfig.json`
- Create: `websearch/vite.config.ts`
- Create: `websearch/index.html`
- Create: `websearch/src/main.tsx`
- Create: `websearch/src/App.tsx`
- Create: `websearch/src/vite-env.d.ts`

**Step 1: Create package.json**

```json
{
  "name": "websearch",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview",
    "lint": "eslint ."
  },
  "dependencies": {
    "react": "^18.2.0",
    "react-dom": "^18.2.0",
    "@radix-ui/react-popover": "^1.1.0",
    "@radix-ui/react-switch": "^1.1.0",
    "lucide-react": "^0.469.0"
  },
  "devDependencies": {
    "@types/react": "^18.2.0",
    "@types/react-dom": "^18.2.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.6.0",
    "vite": "^6.0.0",
    "tailwindcss": "^4.0.0",
    "@tailwindcss/vite": "^4.0.0"
  }
}
```

**Step 2: Create tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "paths": {
      "@/*": ["./src/*"]
    },
    "baseUrl": "."
  },
  "include": ["src"]
}
```

**Step 3: Create vite.config.ts**

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
})
```

**Step 4: Create index.html**

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" type="image/svg+xml" href="/vite.svg" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>WebSearch</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

**Step 5: Create src/vite-env.d.ts**

```typescript
/// <reference types="vite/client" />
```

**Step 6: Create src/main.tsx**

```tsx
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
```

**Step 7: Create src/App.tsx (placeholder)**

```tsx
export default function App() {
  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)]">
      <h1 className="text-2xl p-8">WebSearch</h1>
    </div>
  )
}
```

**Step 8: Install dependencies**

Run: `cd websearch && npm install`
Expected: Dependencies installed successfully

**Step 9: Verify dev server starts**

Run: `cd websearch && npm run dev`
Expected: Vite dev server starts on localhost

**Step 10: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: initialize Vite + React + TypeScript project"
```

---

## Task 2: Theme System - CSS Variables

**Files:**
- Create: `websearch/src/index.css`

**Step 1: Create index.css with theme variables**

```css
@import "tailwindcss";

:root {
  /* Layout */
  --radius-sm: 8px;
  --radius-md: 12px;
  --radius-lg: 16px;
  --radius-xl: 24px;

  /* Shadows */
  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.05);
  --shadow-md: 0 4px 6px rgba(0, 0, 0, 0.07);
  --shadow-lg: 0 10px 15px rgba(0, 0, 0, 0.1);
  --shadow-xl: 0 20px 25px rgba(0, 0, 0, 0.15);
}

/* Tema 1: Neutral (z.ai-inspirert) */
.theme-neutral {
  --accent: #6b7280;
  --accent-hover: #4b5563;
  --accent-subtle: #9ca3af;
}

.theme-neutral.light {
  --bg-primary: #f4f6f8;
  --bg-secondary: #ffffff;
  --bg-tertiary: #e5e7eb;
  --text-primary: #1a1a1a;
  --text-secondary: #4b5563;
  --text-muted: #9ca3af;
  --border: #e5e7eb;
}

.theme-neutral.dark {
  --bg-primary: #141618;
  --bg-secondary: #1e2022;
  --bg-tertiary: #2a2d30;
  --text-primary: #e5e5e5;
  --text-secondary: #a1a1a1;
  --text-muted: #6b7280;
  --border: #2a2d30;
}

/* Tema 2: Teal (Perplexity-inspirert) */
.theme-teal {
  --accent: #20b2aa;
  --accent-hover: #1a9089;
  --accent-subtle: #5cd5ce;
}

.theme-teal.light {
  --bg-primary: #ffffff;
  --bg-secondary: #f8fafa;
  --bg-tertiary: #e6f3f2;
  --text-primary: #1a1a1a;
  --text-secondary: #4b5563;
  --text-muted: #9ca3af;
  --border: #e5e7eb;
}

.theme-teal.dark {
  --bg-primary: #0f0f0f;
  --bg-secondary: #171717;
  --bg-tertiary: #262626;
  --text-primary: #e5e5e5;
  --text-secondary: #a1a1a1;
  --text-muted: #6b7280;
  --border: #262626;
}

/* Animations */
@keyframes pulse-dot {
  0%,
  80%,
  100% {
    opacity: 0.3;
    transform: scale(0.8);
  }
  40% {
    opacity: 1;
    transform: scale(1);
  }
}

@keyframes fade-in-up {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.animate-fade-in-up {
  animation: fade-in-up 0.3s ease-out forwards;
}

.dot-spinner span {
  animation: pulse-dot 1.4s ease-in-out infinite;
}

.dot-spinner span:nth-child(1) {
  animation-delay: 0ms;
}
.dot-spinner span:nth-child(2) {
  animation-delay: 150ms;
}
.dot-spinner span:nth-child(3) {
  animation-delay: 300ms;
}

/* Default theme */
html {
  @apply theme-teal dark;
}

body {
  background-color: var(--bg-primary);
  color: var(--text-primary);
}
```

**Step 2: Verify theme applies**

Run: `cd websearch && npm run dev`
Expected: Page shows dark teal theme background

**Step 3: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add theme system with CSS variables"
```

---

## Task 3: Theme System - useTheme Hook

**Files:**
- Create: `websearch/src/hooks/useTheme.ts`
- Create: `websearch/src/lib/themes.ts`
- Modify: `websearch/src/App.tsx`

**Step 1: Create lib/themes.ts**

```typescript
export type ThemeName = 'neutral' | 'teal'
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
```

**Step 2: Create hooks/useTheme.ts**

```typescript
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
```

**Step 3: Update App.tsx to use theme**

```tsx
import { useTheme } from '@/hooks/useTheme'

export default function App() {
  const { theme, mode, toggleMode } = useTheme()

  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)]">
      <div className="p-8">
        <h1 className="text-2xl mb-4">WebSearch</h1>
        <p className="text-[var(--text-secondary)] mb-4">
          Theme: {theme} | Mode: {mode}
        </p>
        <button
          onClick={toggleMode}
          className="px-4 py-2 rounded-[var(--radius-md)] bg-[var(--accent)] text-white hover:bg-[var(--accent-hover)]"
        >
          Toggle Mode
        </button>
      </div>
    </div>
  )
}
```

**Step 4: Verify theme toggle works**

Run: `cd websearch && npm run dev`
Expected: Clicking toggle switches between light and dark mode

**Step 5: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add useTheme hook with localStorage persistence"
```

---

## Task 4: TypeScript Types

**Files:**
- Create: `websearch/src/lib/types.ts`

**Step 1: Create lib/types.ts**

```typescript
export interface SearchResult {
  id: string
  title: string
  url: string
  displayDomain: string
  snippet: string
  publishedAt?: string
  faviconUrl?: string
  score: number
}

export interface AnswerModel {
  shortAnswer: string
  keyPoints: string[]
  concepts: string[]
  citations: Record<number, string[]> // keyPoint index → source IDs
  followUps: string[]
  caveats: string[]
  confidence: 'low' | 'medium' | 'high'
}

export type Intent = 'chat' | 'search'
export type LoadingPhase = 'understanding' | 'searching' | 'synthesizing'

export interface SearchState {
  view: 'home' | 'loading' | 'results'
  query: string
  intent: Intent
  intentOverride: Intent | null
  phase: LoadingPhase | null
  results: SearchResult[] | null
  discussions: SearchResult[] | null
  answer: AnswerModel | null
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add TypeScript type definitions"
```

---

## Task 5: Intent Classification

**Files:**
- Create: `websearch/src/lib/intent.ts`

**Step 1: Create lib/intent.ts**

```typescript
import type { Intent } from './types'

const QUESTION_STARTERS = [
  // Norsk
  'hva',
  'hvordan',
  'hvorfor',
  'hvem',
  'hvor',
  'når',
  'hvilken',
  'kan du',
  'forklar',
  'sammenlign',
  'hjelp',
  'anbefal',
  // Engelsk
  'what',
  'how',
  'why',
  'who',
  'where',
  'when',
  'which',
  'explain',
  'compare',
  'help',
  'recommend',
  'tell me',
]

export function classifyIntent(query: string): Intent {
  const q = query.trim().toLowerCase()
  if (!q) return 'search'

  const tokens = q.split(/\s+/)

  // Eksplisitte søke-operatorer → search
  if (/(site:|filetype:|pdf|github|docs|download|vs\b)/i.test(q)) {
    return 'search'
  }

  // Spørsmålstegn → chat
  if (q.includes('?')) return 'chat'

  // Starter med spørreord → chat
  if (QUESTION_STARTERS.some((s) => q.startsWith(s + ' ') || q === s)) {
    return 'chat'
  }

  // Lange setninger (7+ ord) → chat
  if (tokens.length >= 7) return 'chat'

  // Korte keyword-queries (1-4 ord) → search
  if (tokens.length <= 4) return 'search'

  // Default: chat (bedre UX)
  return 'chat'
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add intent classification for chat vs search"
```

---

## Task 6: Mock Data

**Files:**
- Create: `websearch/src/lib/mock-data.ts`

**Step 1: Create lib/mock-data.ts**

```typescript
import type { SearchResult, AnswerModel } from './types'

export const MOCK_RESULTS: SearchResult[] = [
  {
    id: '1',
    title: 'Introduction to React Hooks - Official Docs',
    url: 'https://react.dev/reference/react/hooks',
    displayDomain: 'react.dev',
    snippet:
      'Hooks let you use state and other React features without writing a class. They let you use more of React's features from function components.',
    publishedAt: '2024-01-15',
    faviconUrl: 'https://react.dev/favicon.ico',
    score: 0.98,
  },
  {
    id: '2',
    title: 'A Complete Guide to useEffect - Dan Abramov',
    url: 'https://overreacted.io/a-complete-guide-to-useeffect/',
    displayDomain: 'overreacted.io',
    snippet:
      'useEffect lets you synchronize things outside of the React tree according to our current props and state. Effects run after every render by default.',
    publishedAt: '2023-08-20',
    faviconUrl: 'https://overreacted.io/favicon.ico',
    score: 0.95,
  },
  {
    id: '3',
    title: 'React Hooks Tutorial – useState, useEffect, and How to Create Custom Hooks',
    url: 'https://www.freecodecamp.org/news/react-hooks-tutorial/',
    displayDomain: 'freecodecamp.org',
    snippet:
      'Learn how to use React Hooks in your projects. This tutorial covers useState, useEffect, useContext, useReducer, and how to create your own custom hooks.',
    publishedAt: '2024-02-10',
    faviconUrl: 'https://www.freecodecamp.org/favicon.ico',
    score: 0.92,
  },
  {
    id: '4',
    title: 'Rules of Hooks – React Documentation',
    url: 'https://react.dev/reference/rules/rules-of-hooks',
    displayDomain: 'react.dev',
    snippet:
      'Hooks are JavaScript functions, but you need to follow two rules when using them. Only call Hooks at the top level. Only call Hooks from React functions.',
    publishedAt: '2024-01-10',
    faviconUrl: 'https://react.dev/favicon.ico',
    score: 0.90,
  },
  {
    id: '5',
    title: 'Understanding React Hooks - Stack Overflow Blog',
    url: 'https://stackoverflow.blog/2021/10/react-hooks-guide/',
    displayDomain: 'stackoverflow.blog',
    snippet:
      'React Hooks were introduced in React 16.8. They allow developers to use state and lifecycle methods in functional components without using classes.',
    publishedAt: '2023-10-05',
    faviconUrl: 'https://stackoverflow.com/favicon.ico',
    score: 0.88,
  },
]

export const MOCK_DISCUSSIONS: SearchResult[] = [
  {
    id: 'd1',
    title: 'When should I use useMemo and useCallback? - r/reactjs',
    url: 'https://reddit.com/r/reactjs/comments/abc123',
    displayDomain: 'reddit.com/r/reactjs',
    snippet:
      'I see these hooks everywhere but I\'m not sure when to actually use them. Is it worth memoizing everything or is that premature optimization?',
    score: 0.85,
  },
  {
    id: 'd2',
    title: 'useEffect cleanup function not working as expected',
    url: 'https://stackoverflow.com/questions/12345678',
    displayDomain: 'stackoverflow.com',
    snippet:
      'My cleanup function runs on every render instead of just on unmount. What am I doing wrong with my dependency array?',
    score: 0.82,
  },
]

export const MOCK_ANSWER: AnswerModel = {
  shortAnswer:
    'React Hooks are functions that let you use state and lifecycle features in functional components without writing classes. The most common hooks are useState for managing state and useEffect for side effects.',
  keyPoints: [
    'useState lets you add state to functional components. Call it with an initial value and it returns [currentState, setterFunction].',
    'useEffect runs side effects after render. It replaces componentDidMount, componentDidUpdate, and componentWillUnmount.',
    'Custom hooks let you extract and reuse stateful logic between components. They must start with "use".',
    'Hooks must be called at the top level of your component, never inside loops or conditions.',
    'useCallback and useMemo help optimize performance by memoizing functions and computed values.',
  ],
  concepts: ['useState', 'useEffect', 'useCallback', 'useMemo', 'Custom Hooks', 'Rules of Hooks'],
  citations: {
    0: ['1', '3'],
    1: ['2', '3'],
    2: ['3'],
    3: ['4'],
    4: ['5'],
  },
  followUps: [
    'What is the difference between useEffect and useLayoutEffect?',
    'How do I share state between components with hooks?',
    'When should I use useReducer instead of useState?',
  ],
  caveats: [
    'Hooks only work in functional components, not class components',
    'The dependency array in useEffect requires careful management to avoid bugs',
  ],
  confidence: 'high',
}

export async function simulateSearch(query: string): Promise<{
  results: SearchResult[]
  discussions: SearchResult[]
  answer: AnswerModel
}> {
  // Simulate network delay
  await new Promise((resolve) => setTimeout(resolve, 1500))

  return {
    results: MOCK_RESULTS,
    discussions: MOCK_DISCUSSIONS,
    answer: {
      ...MOCK_ANSWER,
      shortAnswer: `Here's what I found about "${query}": ${MOCK_ANSWER.shortAnswer}`,
    },
  }
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add mock data for search results and answers"
```

---

## Task 7: UI Primitives - Button

**Files:**
- Create: `websearch/src/components/ui/Button.tsx`

**Step 1: Create Button component**

```tsx
import { ButtonHTMLAttributes, forwardRef } from 'react'

type ButtonVariant = 'primary' | 'secondary' | 'ghost'
type ButtonSize = 'sm' | 'md' | 'lg'

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant
  size?: ButtonSize
}

const variantStyles: Record<ButtonVariant, string> = {
  primary: 'bg-[var(--accent)] text-white hover:bg-[var(--accent-hover)]',
  secondary:
    'bg-[var(--bg-tertiary)] text-[var(--text-primary)] hover:bg-[var(--border)]',
  ghost:
    'bg-transparent text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)]',
}

const sizeStyles: Record<ButtonSize, string> = {
  sm: 'px-3 py-1.5 text-sm',
  md: 'px-4 py-2 text-base',
  lg: 'px-6 py-3 text-lg',
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'primary', size = 'md', className = '', children, ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={`
          inline-flex items-center justify-center
          rounded-[var(--radius-md)]
          font-medium
          transition-colors duration-150
          focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-2
          disabled:opacity-50 disabled:cursor-not-allowed
          ${variantStyles[variant]}
          ${sizeStyles[size]}
          ${className}
        `}
        {...props}
      >
        {children}
      </button>
    )
  }
)

Button.displayName = 'Button'
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add Button UI primitive"
```

---

## Task 8: UI Primitives - Input

**Files:**
- Create: `websearch/src/components/ui/Input.tsx`

**Step 1: Create Input component**

```tsx
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
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add Input UI primitive with icon support"
```

---

## Task 9: UI Primitives - Card, Chip, Skeleton

**Files:**
- Create: `websearch/src/components/ui/Card.tsx`
- Create: `websearch/src/components/ui/Chip.tsx`
- Create: `websearch/src/components/ui/Skeleton.tsx`

**Step 1: Create Card component**

```tsx
import { HTMLAttributes, forwardRef } from 'react'

interface CardProps extends HTMLAttributes<HTMLDivElement> {
  hover?: boolean
}

export const Card = forwardRef<HTMLDivElement, CardProps>(
  ({ hover = false, className = '', children, ...props }, ref) => {
    return (
      <div
        ref={ref}
        className={`
          rounded-[var(--radius-lg)]
          bg-[var(--bg-secondary)]
          border border-[var(--border)]
          shadow-[var(--shadow-sm)]
          ${hover ? 'transition-shadow hover:shadow-[var(--shadow-md)]' : ''}
          ${className}
        `}
        {...props}
      >
        {children}
      </div>
    )
  }
)

Card.displayName = 'Card'
```

**Step 2: Create Chip component**

```tsx
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
```

**Step 3: Create Skeleton component**

```tsx
import { HTMLAttributes } from 'react'

interface SkeletonProps extends HTMLAttributes<HTMLDivElement> {
  width?: string | number
  height?: string | number
}

export function Skeleton({ width, height, className = '', style, ...props }: SkeletonProps) {
  return (
    <div
      className={`animate-pulse bg-[var(--bg-tertiary)] rounded-[var(--radius-sm)] ${className}`}
      style={{
        width: typeof width === 'number' ? `${width}px` : width,
        height: typeof height === 'number' ? `${height}px` : height,
        ...style,
      }}
      {...props}
    />
  )
}
```

**Step 4: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add Card, Chip, and Skeleton UI primitives"
```

---

## Task 10: UI Primitives - Radix Wrappers (Popover, Switch)

**Files:**
- Create: `websearch/src/components/ui/Popover.tsx`
- Create: `websearch/src/components/ui/Switch.tsx`

**Step 1: Create Popover wrapper**

```tsx
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
```

**Step 2: Create Switch wrapper**

```tsx
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
```

**Step 3: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add Radix UI wrappers for Popover and Switch"
```

---

## Task 11: UI Primitives Index Export

**Files:**
- Create: `websearch/src/components/ui/index.ts`

**Step 1: Create barrel export**

```typescript
export { Button } from './Button'
export { Input } from './Input'
export { Card } from './Card'
export { Chip } from './Chip'
export { Skeleton } from './Skeleton'
export {
  Popover,
  PopoverTrigger,
  PopoverContent,
  PopoverAnchor,
  PopoverPortal,
  PopoverArrow,
} from './Popover'
export { Switch } from './Switch'
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add UI primitives barrel export"
```

---

## Task 12: useStreamingText Hook

**Files:**
- Create: `websearch/src/hooks/useStreamingText.ts`

**Step 1: Create useStreamingText hook**

```typescript
import { useState, useEffect } from 'react'

interface UseStreamingTextOptions {
  speed?: number // ms per character
  delay?: number // initial delay before starting
}

interface UseStreamingTextReturn {
  displayed: string
  isComplete: boolean
}

export function useStreamingText(
  fullText: string,
  options?: UseStreamingTextOptions
): UseStreamingTextReturn {
  const { speed = 20, delay = 0 } = options ?? {}
  const [displayed, setDisplayed] = useState('')
  const [isComplete, setIsComplete] = useState(false)

  useEffect(() => {
    setDisplayed('')
    setIsComplete(false)

    if (!fullText) {
      setIsComplete(true)
      return
    }

    let i = 0
    let intervalId: number | undefined

    const timeoutId = setTimeout(() => {
      intervalId = window.setInterval(() => {
        if (i <= fullText.length) {
          setDisplayed(fullText.slice(0, i))
          i++
        } else {
          setIsComplete(true)
          if (intervalId) clearInterval(intervalId)
        }
      }, speed)
    }, delay)

    return () => {
      clearTimeout(timeoutId)
      if (intervalId) clearInterval(intervalId)
    }
  }, [fullText, speed, delay])

  return { displayed, isComplete }
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add useStreamingText hook for text animation"
```

---

## Task 13: useSearch Hook (State Machine)

**Files:**
- Create: `websearch/src/hooks/useSearch.ts`

**Step 1: Create useSearch hook**

```typescript
import { useState, useCallback, useEffect } from 'react'
import type { SearchState, LoadingPhase, Intent, SearchResult, AnswerModel } from '@/lib/types'
import { classifyIntent } from '@/lib/intent'
import { simulateSearch } from '@/lib/mock-data'

const LOADING_PHASES: { key: LoadingPhase; duration: number }[] = [
  { key: 'understanding', duration: 300 },
  { key: 'searching', duration: 1000 },
  { key: 'synthesizing', duration: 200 },
]

const initialState: SearchState = {
  view: 'home',
  query: '',
  intent: 'search',
  intentOverride: null,
  phase: null,
  results: null,
  discussions: null,
  answer: null,
}

export function useSearch() {
  const [state, setState] = useState<SearchState>(initialState)

  // Update URL when query changes
  useEffect(() => {
    if (state.view === 'results' && state.query) {
      const url = new URL(window.location.href)
      url.searchParams.set('q', state.query)
      window.history.replaceState({}, '', url.toString())
    } else if (state.view === 'home') {
      const url = new URL(window.location.href)
      url.searchParams.delete('q')
      window.history.replaceState({}, '', url.toString())
    }
  }, [state.view, state.query])

  // Check URL on mount for initial query
  useEffect(() => {
    const url = new URL(window.location.href)
    const q = url.searchParams.get('q')
    if (q) {
      search(q)
    }
  }, [])

  const search = useCallback(async (query: string) => {
    if (!query.trim()) return

    const intent = classifyIntent(query)

    setState((s) => ({
      ...s,
      view: 'loading',
      query,
      intent,
      phase: 'understanding',
      results: null,
      discussions: null,
      answer: null,
    }))

    // Progress through loading phases
    let phaseIndex = 0
    const phaseInterval = setInterval(() => {
      phaseIndex++
      if (phaseIndex < LOADING_PHASES.length) {
        setState((s) => ({
          ...s,
          phase: LOADING_PHASES[phaseIndex].key,
        }))
      }
    }, LOADING_PHASES[phaseIndex].duration)

    try {
      const data = await simulateSearch(query)

      clearInterval(phaseInterval)

      setState((s) => ({
        ...s,
        view: 'results',
        phase: null,
        results: data.results,
        discussions: data.discussions,
        answer: data.answer,
      }))
    } catch (error) {
      clearInterval(phaseInterval)
      // For now, just go back to home on error
      setState(initialState)
    }
  }, [])

  const setIntentOverride = useCallback((override: Intent | null) => {
    setState((s) => ({ ...s, intentOverride: override }))
  }, [])

  const reset = useCallback(() => {
    setState(initialState)
  }, [])

  const effectiveIntent = state.intentOverride ?? state.intent

  return {
    ...state,
    effectiveIntent,
    search,
    setIntentOverride,
    reset,
  }
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add useSearch hook with state machine"
```

---

## Task 14: Hooks Index Export

**Files:**
- Create: `websearch/src/hooks/index.ts`

**Step 1: Create barrel export**

```typescript
export { useTheme } from './useTheme'
export { useStreamingText } from './useStreamingText'
export { useSearch } from './useSearch'
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add hooks barrel export"
```

---

## Task 15: Loading Components

**Files:**
- Create: `websearch/src/features/loading/LoadingSpinner.tsx`
- Create: `websearch/src/features/loading/SkeletonResults.tsx`
- Create: `websearch/src/features/loading/LoadingPage.tsx`
- Create: `websearch/src/features/loading/index.ts`

**Step 1: Create LoadingSpinner**

```tsx
export function LoadingSpinner() {
  return (
    <div className="dot-spinner flex items-center gap-1.5">
      <span className="w-2 h-2 rounded-full bg-[var(--accent)]" />
      <span className="w-2 h-2 rounded-full bg-[var(--accent)]" />
      <span className="w-2 h-2 rounded-full bg-[var(--accent)]" />
    </div>
  )
}
```

**Step 2: Create SkeletonResults**

```tsx
import { Skeleton } from '@/components/ui'

export function SkeletonResults() {
  return (
    <div className="grid grid-cols-3 gap-4 max-w-4xl mx-auto">
      {[1, 2, 3].map((i) => (
        <div key={i} className="p-4 rounded-[var(--radius-lg)] bg-[var(--bg-secondary)]">
          <Skeleton height={16} className="w-3/4 mb-3" />
          <Skeleton height={12} className="w-1/2 mb-2" />
          <Skeleton height={12} className="w-full mb-1" />
          <Skeleton height={12} className="w-5/6" />
        </div>
      ))}
    </div>
  )
}
```

**Step 3: Create LoadingPage**

```tsx
import type { LoadingPhase } from '@/lib/types'
import { LoadingSpinner } from './LoadingSpinner'
import { SkeletonResults } from './SkeletonResults'

interface LoadingPageProps {
  query: string
  phase: LoadingPhase | null
}

const PHASE_LABELS: Record<LoadingPhase, string> = {
  understanding: 'Understanding query...',
  searching: 'Searching sources...',
  synthesizing: 'Synthesizing answer...',
}

export function LoadingPage({ query, phase }: LoadingPageProps) {
  return (
    <div className="min-h-screen flex flex-col">
      {/* TopBar placeholder */}
      <header className="sticky top-0 z-10 px-6 py-4 bg-[var(--bg-primary)] border-b border-[var(--border)]">
        <div className="max-w-6xl mx-auto flex items-center gap-4">
          <span className="text-xl font-semibold text-[var(--accent)]">WebSearch</span>
          <div className="flex-1 max-w-xl">
            <div className="h-11 px-4 rounded-[var(--radius-lg)] bg-[var(--bg-secondary)] border border-[var(--border)] flex items-center">
              <span className="text-[var(--text-muted)]">{query}</span>
            </div>
          </div>
        </div>
      </header>

      {/* Loading content */}
      <main className="flex-1 flex flex-col items-center justify-center gap-6 p-8">
        <LoadingSpinner />
        <p className="text-lg text-[var(--text-secondary)]">
          {phase ? PHASE_LABELS[phase] : 'Loading...'}
        </p>
        <SkeletonResults />
      </main>
    </div>
  )
}
```

**Step 4: Create index export**

```typescript
export { LoadingPage } from './LoadingPage'
export { LoadingSpinner } from './LoadingSpinner'
export { SkeletonResults } from './SkeletonResults'
```

**Step 5: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add loading page with spinner and skeletons"
```

---

## Task 16: Home Page Components

**Files:**
- Create: `websearch/src/features/home/SearchHero.tsx`
- Create: `websearch/src/features/home/QuickActions.tsx`
- Create: `websearch/src/features/home/HomePage.tsx`
- Create: `websearch/src/features/home/index.ts`

**Step 1: Create SearchHero**

```tsx
import { useState, useRef, useEffect, KeyboardEvent } from 'react'
import { Search } from 'lucide-react'
import { Input } from '@/components/ui'

interface SearchHeroProps {
  onSearch: (query: string) => void
}

export function SearchHero({ onSearch }: SearchHeroProps) {
  const [query, setQuery] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  // Auto-focus on mount
  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  // Hotkey: "/" focuses input
  useEffect(() => {
    const handleKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key === '/' && document.activeElement !== inputRef.current) {
        e.preventDefault()
        inputRef.current?.focus()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [])

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && query.trim()) {
      onSearch(query.trim())
    }
  }

  return (
    <div className="flex flex-col items-center gap-6">
      <h1 className="text-4xl md:text-5xl font-bold text-[var(--text-primary)]">
        What do you want to know?
      </h1>
      <p className="text-lg text-[var(--text-secondary)]">
        Search the web and get AI-powered answers
      </p>
      <div className="w-full max-w-2xl">
        <Input
          ref={inputRef}
          size="lg"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Ask anything..."
          leftIcon={<Search className="w-5 h-5" />}
        />
      </div>
      <p className="text-sm text-[var(--text-muted)]">
        Press <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-tertiary)] font-mono">/</kbd> to
        focus
      </p>
    </div>
  )
}
```

**Step 2: Create QuickActions**

```tsx
import { Chip } from '@/components/ui'
import { Sparkles, Code, FileText, Image } from 'lucide-react'

interface QuickActionsProps {
  onAction: (query: string) => void
}

const QUICK_ACTIONS = [
  { label: 'AI Overview', icon: Sparkles, query: 'What is artificial intelligence?' },
  { label: 'Write Code', icon: Code, query: 'How do I write a React hook?' },
  { label: 'Summarize', icon: FileText, query: 'Summarize the latest tech news' },
  { label: 'Create Image', icon: Image, query: 'How do I create images with AI?' },
]

export function QuickActions({ onAction }: QuickActionsProps) {
  return (
    <div className="flex flex-wrap justify-center gap-2">
      {QUICK_ACTIONS.map(({ label, icon: Icon, query }) => (
        <Chip key={label} onClick={() => onAction(query)}>
          <Icon className="w-4 h-4 mr-1.5" />
          {label}
        </Chip>
      ))}
    </div>
  )
}
```

**Step 3: Create HomePage**

```tsx
import { SearchHero } from './SearchHero'
import { QuickActions } from './QuickActions'

interface HomePageProps {
  onSearch: (query: string) => void
}

export function HomePage({ onSearch }: HomePageProps) {
  return (
    <div className="min-h-screen flex flex-col items-center justify-center p-8">
      <div className="w-full max-w-3xl flex flex-col items-center gap-8">
        <SearchHero onSearch={onSearch} />
        <QuickActions onAction={onSearch} />
      </div>
    </div>
  )
}
```

**Step 4: Create index export**

```typescript
export { HomePage } from './HomePage'
export { SearchHero } from './SearchHero'
export { QuickActions } from './QuickActions'
```

**Step 5: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add home page with search hero and quick actions"
```

---

## Task 17: Results Page - TopBar

**Files:**
- Create: `websearch/src/features/search/TopBar.tsx`

**Step 1: Create TopBar**

```tsx
import { useState, KeyboardEvent } from 'react'
import { Search, Moon, Sun } from 'lucide-react'
import { Input, Button } from '@/components/ui'
import { useTheme } from '@/hooks'

interface TopBarProps {
  query: string
  onSearch: (query: string) => void
}

export function TopBar({ query: initialQuery, onSearch }: TopBarProps) {
  const [query, setQuery] = useState(initialQuery)
  const { mode, toggleMode } = useTheme()

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && query.trim()) {
      onSearch(query.trim())
    }
  }

  return (
    <header className="sticky top-0 z-10 px-6 py-3 bg-[var(--bg-primary)] border-b border-[var(--border)]">
      <div className="max-w-6xl mx-auto flex items-center gap-4">
        <button
          onClick={() => window.location.reload()}
          className="text-xl font-semibold text-[var(--accent)] hover:opacity-80 transition-opacity"
        >
          WebSearch
        </button>

        <div className="flex-1 max-w-xl">
          <Input
            size="md"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Search..."
            leftIcon={<Search className="w-4 h-4" />}
          />
        </div>

        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={toggleMode}>
            {mode === 'dark' ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
          </Button>
        </div>
      </div>
    </header>
  )
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add TopBar component for results page"
```

---

## Task 18: Results Page - ResultCard

**Files:**
- Create: `websearch/src/features/search/ResultCard.tsx`

**Step 1: Create ResultCard**

```tsx
import type { SearchResult } from '@/lib/types'
import { Card } from '@/components/ui'
import { ExternalLink } from 'lucide-react'

interface ResultCardProps {
  result: SearchResult
  compact?: boolean
}

export function ResultCard({ result, compact = false }: ResultCardProps) {
  return (
    <Card hover className="p-4">
      <a
        href={result.url}
        target="_blank"
        rel="noopener noreferrer"
        className="block group"
      >
        <div className="flex items-start gap-3">
          {result.faviconUrl && (
            <img
              src={result.faviconUrl}
              alt=""
              className="w-4 h-4 mt-1 rounded"
              onError={(e) => {
                e.currentTarget.style.display = 'none'
              }}
            />
          )}
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <span className="text-xs text-[var(--text-muted)] truncate">
                {result.displayDomain}
              </span>
              <ExternalLink className="w-3 h-3 text-[var(--text-muted)] opacity-0 group-hover:opacity-100 transition-opacity" />
            </div>
            <h3 className="font-medium text-[var(--accent)] group-hover:underline line-clamp-2">
              {result.title}
            </h3>
            {!compact && (
              <p className="mt-1 text-sm text-[var(--text-secondary)] line-clamp-2">
                {result.snippet}
              </p>
            )}
            {result.publishedAt && !compact && (
              <span className="mt-2 text-xs text-[var(--text-muted)]">
                {result.publishedAt}
              </span>
            )}
          </div>
        </div>
      </a>
    </Card>
  )
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add ResultCard component"
```

---

## Task 19: Results Page - ResultsList and DiscussionsList

**Files:**
- Create: `websearch/src/features/search/ResultsList.tsx`
- Create: `websearch/src/features/search/DiscussionsList.tsx`

**Step 1: Create ResultsList**

```tsx
import type { SearchResult } from '@/lib/types'
import { ResultCard } from './ResultCard'

interface ResultsListProps {
  results: SearchResult[]
  variant?: 'full' | 'compact'
}

export function ResultsList({ results, variant = 'full' }: ResultsListProps) {
  const displayResults = variant === 'compact' ? results.slice(0, 4) : results

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide">
        Web Results
      </h2>
      {displayResults.map((result) => (
        <ResultCard key={result.id} result={result} compact={variant === 'compact'} />
      ))}
    </div>
  )
}
```

**Step 2: Create DiscussionsList**

```tsx
import type { SearchResult } from '@/lib/types'
import { ResultCard } from './ResultCard'

interface DiscussionsListProps {
  discussions: SearchResult[]
}

export function DiscussionsList({ discussions }: DiscussionsListProps) {
  if (!discussions.length) return null

  return (
    <div className="flex flex-col gap-3 mt-6">
      <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide">
        Discussions
      </h2>
      {discussions.map((result) => (
        <ResultCard key={result.id} result={result} />
      ))}
    </div>
  )
}
```

**Step 3: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add ResultsList and DiscussionsList components"
```

---

## Task 20: Results Page - CitationBadge

**Files:**
- Create: `websearch/src/features/search/CitationBadge.tsx`

**Step 1: Create CitationBadge**

```tsx
interface CitationBadgeProps {
  number: number
  onClick?: () => void
}

export function CitationBadge({ number, onClick }: CitationBadgeProps) {
  return (
    <button
      onClick={onClick}
      className="
        inline-flex items-center justify-center
        w-5 h-5
        rounded-full
        bg-[var(--accent)]/20
        text-[var(--accent)]
        text-xs font-medium
        hover:bg-[var(--accent)]/30
        transition-colors
      "
    >
      {number}
    </button>
  )
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add CitationBadge component"
```

---

## Task 21: Results Page - AnswerPanel

**Files:**
- Create: `websearch/src/features/search/AnswerPanel.tsx`

**Step 1: Create AnswerPanel**

```tsx
import type { AnswerModel } from '@/lib/types'
import { useStreamingText } from '@/hooks'
import { Card, Chip } from '@/components/ui'
import { CitationBadge } from './CitationBadge'
import { Sparkles, AlertCircle, MessageSquare } from 'lucide-react'

interface AnswerPanelProps {
  answer: AnswerModel
  variant?: 'full' | 'compact'
}

export function AnswerPanel({ answer, variant = 'full' }: AnswerPanelProps) {
  const { displayed: shortAnswer, isComplete } = useStreamingText(answer.shortAnswer, {
    delay: 200,
  })

  const displayKeyPoints = variant === 'compact' ? answer.keyPoints.slice(0, 3) : answer.keyPoints
  const displayFollowUps = variant === 'compact' ? answer.followUps.slice(0, 2) : answer.followUps

  return (
    <Card className="p-6">
      {/* Header */}
      <div className="flex items-center gap-2 mb-4">
        <Sparkles className="w-5 h-5 text-[var(--accent)]" />
        <h2 className="font-semibold text-[var(--text-primary)]">AI Answer</h2>
        <span
          className={`
            ml-auto px-2 py-0.5 rounded-full text-xs font-medium
            ${
              answer.confidence === 'high'
                ? 'bg-green-500/20 text-green-400'
                : answer.confidence === 'medium'
                  ? 'bg-yellow-500/20 text-yellow-400'
                  : 'bg-red-500/20 text-red-400'
            }
          `}
        >
          {answer.confidence} confidence
        </span>
      </div>

      {/* Short Answer */}
      <p className="text-lg text-[var(--text-primary)] mb-4 leading-relaxed">
        {shortAnswer}
        {!isComplete && <span className="animate-pulse">|</span>}
      </p>

      {/* Key Points */}
      {isComplete && (
        <div className="animate-fade-in-up">
          <h3 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">
            Key Points
          </h3>
          <ul className="space-y-3 mb-6">
            {displayKeyPoints.map((point, i) => (
              <li
                key={i}
                className="flex items-start gap-3 text-[var(--text-secondary)]"
                style={{ animationDelay: `${i * 100}ms` }}
              >
                <span className="w-1.5 h-1.5 rounded-full bg-[var(--accent)] mt-2 flex-shrink-0" />
                <span className="flex-1">{point}</span>
                {answer.citations[i] && (
                  <div className="flex gap-1">
                    {answer.citations[i].map((sourceId, j) => (
                      <CitationBadge key={j} number={parseInt(sourceId)} />
                    ))}
                  </div>
                )}
              </li>
            ))}
          </ul>

          {/* Concepts */}
          {variant === 'full' && answer.concepts.length > 0 && (
            <div className="mb-6">
              <h3 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2">
                Related Concepts
              </h3>
              <div className="flex flex-wrap gap-2">
                {answer.concepts.map((concept) => (
                  <Chip key={concept}>{concept}</Chip>
                ))}
              </div>
            </div>
          )}

          {/* Caveats */}
          {variant === 'full' && answer.caveats.length > 0 && (
            <div className="mb-6 p-3 rounded-[var(--radius-md)] bg-yellow-500/10 border border-yellow-500/20">
              <div className="flex items-center gap-2 mb-2">
                <AlertCircle className="w-4 h-4 text-yellow-400" />
                <h3 className="text-sm font-semibold text-yellow-400">Important Notes</h3>
              </div>
              <ul className="space-y-1">
                {answer.caveats.map((caveat, i) => (
                  <li key={i} className="text-sm text-yellow-200/80">
                    {caveat}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Follow-ups */}
          {displayFollowUps.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-2 flex items-center gap-2">
                <MessageSquare className="w-4 h-4" />
                Follow-up Questions
              </h3>
              <div className="flex flex-col gap-2">
                {displayFollowUps.map((question, i) => (
                  <button
                    key={i}
                    className="
                      text-left px-3 py-2
                      rounded-[var(--radius-md)]
                      bg-[var(--bg-tertiary)]
                      text-[var(--text-secondary)]
                      hover:bg-[var(--border)]
                      transition-colors
                      text-sm
                    "
                  >
                    {question}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </Card>
  )
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add AnswerPanel with streaming text and citations"
```

---

## Task 22: Results Page - ResultsLayout

**Files:**
- Create: `websearch/src/features/search/ResultsLayout.tsx`

**Step 1: Create ResultsLayout**

```tsx
import type { ReactNode } from 'react'
import type { Intent } from '@/lib/types'

interface ResultsLayoutProps {
  intent: Intent
  answerPanel: ReactNode
  serpPanel: ReactNode
}

export function ResultsLayout({ intent, answerPanel, serpPanel }: ResultsLayoutProps) {
  const isChat = intent === 'chat'

  return (
    <div
      className={`
        grid gap-6 transition-all duration-300
        ${isChat ? 'lg:grid-cols-[65fr_35fr]' : 'lg:grid-cols-[60fr_40fr]'}
        grid-cols-1
      `}
    >
      {isChat ? (
        <>
          {answerPanel}
          {serpPanel}
        </>
      ) : (
        <>
          {serpPanel}
          {answerPanel}
        </>
      )}
    </div>
  )
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add ResultsLayout with adaptive grid"
```

---

## Task 23: Results Page - IntentToggle

**Files:**
- Create: `websearch/src/components/composed/IntentToggle.tsx`

**Step 1: Create IntentToggle**

```tsx
import type { Intent } from '@/lib/types'
import { Sparkles, Search } from 'lucide-react'

interface IntentToggleProps {
  intent: Intent
  onChange: (intent: Intent) => void
}

export function IntentToggle({ intent, onChange }: IntentToggleProps) {
  return (
    <div className="inline-flex rounded-full bg-[var(--bg-tertiary)] p-1">
      <button
        onClick={() => onChange('chat')}
        className={`
          flex items-center gap-1.5 px-3 py-1.5 rounded-full text-sm font-medium transition-colors
          ${
            intent === 'chat'
              ? 'bg-[var(--accent)] text-white'
              : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
          }
        `}
      >
        <Sparkles className="w-4 h-4" />
        Answer
      </button>
      <button
        onClick={() => onChange('search')}
        className={`
          flex items-center gap-1.5 px-3 py-1.5 rounded-full text-sm font-medium transition-colors
          ${
            intent === 'search'
              ? 'bg-[var(--accent)] text-white'
              : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
          }
        `}
      >
        <Search className="w-4 h-4" />
        Search
      </button>
    </div>
  )
}
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add IntentToggle component"
```

---

## Task 24: Results Page - SearchPage Container

**Files:**
- Create: `websearch/src/features/search/SearchPage.tsx`
- Create: `websearch/src/features/search/index.ts`

**Step 1: Create SearchPage**

```tsx
import type { SearchResult, AnswerModel, Intent } from '@/lib/types'
import { TopBar } from './TopBar'
import { ResultsLayout } from './ResultsLayout'
import { ResultsList } from './ResultsList'
import { DiscussionsList } from './DiscussionsList'
import { AnswerPanel } from './AnswerPanel'
import { IntentToggle } from '@/components/composed/IntentToggle'

interface SearchPageProps {
  query: string
  effectiveIntent: Intent
  results: SearchResult[] | null
  discussions: SearchResult[] | null
  answer: AnswerModel | null
  onNewSearch: (query: string) => void
  setIntentOverride: (intent: Intent | null) => void
}

export function SearchPage({
  query,
  effectiveIntent,
  results,
  discussions,
  answer,
  onNewSearch,
  setIntentOverride,
}: SearchPageProps) {
  const serpVariant = effectiveIntent === 'search' ? 'full' : 'compact'
  const answerVariant = effectiveIntent === 'chat' ? 'full' : 'compact'

  return (
    <div className="min-h-screen flex flex-col">
      <TopBar query={query} onSearch={onNewSearch} />

      <main className="flex-1 px-6 py-6">
        <div className="max-w-6xl mx-auto">
          {/* Intent Toggle */}
          <div className="mb-6 flex justify-center">
            <IntentToggle
              intent={effectiveIntent}
              onChange={(intent) => setIntentOverride(intent)}
            />
          </div>

          {/* Results Layout */}
          <ResultsLayout
            intent={effectiveIntent}
            answerPanel={answer && <AnswerPanel answer={answer} variant={answerVariant} />}
            serpPanel={
              <div>
                {results && <ResultsList results={results} variant={serpVariant} />}
                {discussions && serpVariant === 'full' && (
                  <DiscussionsList discussions={discussions} />
                )}
              </div>
            }
          />
        </div>
      </main>
    </div>
  )
}
```

**Step 2: Create index export**

```typescript
export { SearchPage } from './SearchPage'
export { TopBar } from './TopBar'
export { ResultsLayout } from './ResultsLayout'
export { ResultsList } from './ResultsList'
export { ResultCard } from './ResultCard'
export { DiscussionsList } from './DiscussionsList'
export { AnswerPanel } from './AnswerPanel'
export { CitationBadge } from './CitationBadge'
```

**Step 3: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add SearchPage container with full results view"
```

---

## Task 25: Composed Components Index

**Files:**
- Create: `websearch/src/components/composed/index.ts`

**Step 1: Create barrel export**

```typescript
export { IntentToggle } from './IntentToggle'
```

**Step 2: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: add composed components barrel export"
```

---

## Task 26: Wire Up App.tsx

**Files:**
- Modify: `websearch/src/App.tsx`

**Step 1: Update App.tsx to use all components**

```tsx
import { useSearch, useTheme } from '@/hooks'
import { HomePage } from '@/features/home'
import { LoadingPage } from '@/features/loading'
import { SearchPage } from '@/features/search'

export default function App() {
  // Initialize theme
  useTheme()

  const search = useSearch()

  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)]">
      {search.view === 'home' && <HomePage onSearch={search.search} />}

      {search.view === 'loading' && (
        <LoadingPage query={search.query} phase={search.phase} />
      )}

      {search.view === 'results' && (
        <SearchPage
          query={search.query}
          effectiveIntent={search.effectiveIntent}
          results={search.results}
          discussions={search.discussions}
          answer={search.answer}
          onNewSearch={search.search}
          setIntentOverride={search.setIntentOverride}
        />
      )}
    </div>
  )
}
```

**Step 2: Verify app works end-to-end**

Run: `cd websearch && npm run dev`
Expected: Full flow works - home → loading → results with adaptive layout

**Step 3: Commit**

```bash
cd websearch && git add -A && git commit -m "feat: wire up App.tsx with all views"
```

---

## Task 27: Build Verification

**Files:** None (verification only)

**Step 1: Run TypeScript type check**

Run: `cd websearch && npx tsc --noEmit`
Expected: No type errors

**Step 2: Run production build**

Run: `cd websearch && npm run build`
Expected: Build succeeds with no errors

**Step 3: Preview production build**

Run: `cd websearch && npm run preview`
Expected: Production build runs correctly

**Step 4: Final commit**

```bash
cd websearch && git add -A && git commit -m "chore: verify build and types"
```

---

## Summary

This plan implements Phase 1 (MVP) of the WebSearch frontend:

1. **Tasks 1-3:** Project setup with Vite, React, TypeScript, and theme system
2. **Tasks 4-6:** Type definitions, intent classification, and mock data
3. **Tasks 7-11:** UI primitives (Button, Input, Card, Chip, Skeleton, Popover, Switch)
4. **Tasks 12-14:** Hooks (useStreamingText, useSearch state machine)
5. **Tasks 15-16:** Loading and Home page features
6. **Tasks 17-25:** Results page with adaptive layout, SERP, and AI answer panel
7. **Tasks 26-27:** Final wiring and build verification

Each task is a small, focused unit with explicit file paths, code, and verification steps.
