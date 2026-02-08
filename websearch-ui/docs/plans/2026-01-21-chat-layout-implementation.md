# Chat Layout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implementer Z.ai-stil chat layout som alternativ til classic layout, med SearchComposer og hero→sticky transformasjon.

**Architecture:** Ny `SearchComposer` komponent med 3 varianter, `chat-layout` feature med egne pages, `useScrollComposer` hook for sticky-trigger. Classic layout flyttes til egen feature-mappe.

**Tech Stack:** React 18, TypeScript, Tailwind CSS, Lucide React

---

## Task 1: SearchComposer - Grunnstruktur

**Files:**
- Create: `src/components/composed/SearchComposer.tsx`

**Step 1: Create SearchComposer with hero variant**

```tsx
import { useRef, useEffect, KeyboardEvent } from 'react'
import { Plus, Search, Globe, ArrowUp } from 'lucide-react'

type ComposerVariant = 'hero' | 'inline' | 'sticky'
type SearchMode = 'search' | 'deepThink'

interface SearchComposerProps {
  variant: ComposerVariant
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  mode: SearchMode
  onModeChange: (mode: SearchMode) => void
  disabled?: boolean
  showQuickActions?: boolean
  autoFocus?: boolean
}

const wrapperStyles: Record<ComposerVariant, string> = {
  hero: `
    max-w-2xl w-full mx-auto
    rounded-2xl
    border border-[var(--border)]
    bg-[var(--bg-secondary)]
    shadow-[0px_4px_16px_0px_rgba(0,0,0,0.1)]
  `,
  inline: `
    max-w-3xl w-full
    rounded-xl
    border border-[var(--border)]
    bg-[var(--bg-secondary)]
    shadow-[0px_2px_8px_0px_rgba(0,0,0,0.08)]
  `,
  sticky: `
    flex-1 max-w-xl
    rounded-lg
    border border-[var(--border)]
    bg-[var(--bg-secondary)]
  `,
}

const textareaStyles: Record<ComposerVariant, string> = {
  hero: 'min-h-[72px] text-base p-4',
  inline: 'min-h-[56px] text-sm p-3',
  sticky: 'h-10 text-sm px-3 py-2',
}

export function SearchComposer({
  variant,
  value,
  onChange,
  onSubmit,
  mode,
  onModeChange,
  disabled = false,
  autoFocus = false,
}: SearchComposerProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return

    textarea.style.height = 'auto'
    const maxHeight = variant === 'sticky' ? 40 : 144
    textarea.style.height = `${Math.min(textarea.scrollHeight, maxHeight)}px`
  }, [value, variant])

  // Auto-focus
  useEffect(() => {
    if (autoFocus) {
      textareaRef.current?.focus()
    }
  }, [autoFocus])

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (value.trim() && !disabled) {
        onSubmit()
      }
    }
  }

  const isSticky = variant === 'sticky'

  return (
    <div className={wrapperStyles[variant]}>
      {/* Textarea area */}
      <div className={isSticky ? 'flex items-center' : ''}>
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Hva vil du vite?"
          disabled={disabled}
          rows={1}
          className={`
            w-full bg-transparent resize-none
            text-[var(--text-primary)]
            placeholder:text-[var(--text-muted)]
            focus:outline-none
            ${textareaStyles[variant]}
          `}
        />

        {/* Sticky: send button inline */}
        {isSticky && (
          <button
            onClick={onSubmit}
            disabled={!value.trim() || disabled}
            className={`
              p-2 mr-1 rounded-lg transition-colors shrink-0
              ${value.trim() && !disabled
                ? 'bg-[var(--accent)] text-white'
                : 'bg-[var(--bg-tertiary)] text-[var(--text-muted)]'}
            `}
          >
            <ArrowUp className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Bottom controls - not for sticky */}
      {!isSticky && (
        <div className="flex items-center justify-between px-3 pb-3 mt-1">
          {/* Left: attach + mode pills */}
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="p-1.5 rounded-lg hover:bg-[var(--bg-tertiary)] text-[var(--text-secondary)]"
            >
              <Plus className="w-5 h-5" />
            </button>

            <ModePill
              icon={<Search className="w-4 h-4" />}
              label="Search"
              active={mode === 'search'}
              onClick={() => onModeChange('search')}
            />

            <ModePill
              icon={<Globe className="w-4 h-4" />}
              label="Deep Think"
              active={mode === 'deepThink'}
              onClick={() => onModeChange('deepThink')}
            />
          </div>

          {/* Right: send */}
          <button
            onClick={onSubmit}
            disabled={!value.trim() || disabled}
            className={`
              p-2 rounded-lg transition-colors
              ${value.trim() && !disabled
                ? 'bg-[var(--accent)] text-white'
                : 'bg-[var(--bg-tertiary)] text-[var(--text-muted)]'}
            `}
          >
            <ArrowUp className="w-4 h-4" />
          </button>
        </div>
      )}
    </div>
  )
}

interface ModePillProps {
  icon: React.ReactNode
  label: string
  active: boolean
  onClick: () => void
}

function ModePill({ icon, label, active, onClick }: ModePillProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`
        flex items-center gap-1.5 px-2 py-1.5
        text-sm rounded-lg border transition-colors
        ${active
          ? 'bg-[var(--accent)]/10 border-[var(--accent)]/30 text-[var(--accent)]'
          : 'border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)]'}
      `}
    >
      {icon}
      <span className="hidden sm:inline">{label}</span>
    </button>
  )
}
```

**Step 2: Verify file created**

Run: `ls -la src/components/composed/SearchComposer.tsx`

**Step 3: Commit**

```bash
git add src/components/composed/SearchComposer.tsx
git commit -m "feat: add SearchComposer with hero/inline/sticky variants"
```

---

## Task 2: useScrollComposer Hook

**Files:**
- Create: `src/hooks/useScrollComposer.ts`
- Modify: `src/hooks/index.ts`

**Step 1: Create useScrollComposer hook**

```typescript
import { useRef, useState, useEffect } from 'react'

export function useScrollComposer() {
  const composerRef = useRef<HTMLDivElement>(null)
  const [showSticky, setShowSticky] = useState(false)

  useEffect(() => {
    const el = composerRef.current
    if (!el) return

    const observer = new IntersectionObserver(
      ([entry]) => {
        setShowSticky(!entry.isIntersecting)
      },
      { threshold: 0, rootMargin: '-60px 0px 0px 0px' }
    )

    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  return { composerRef, showSticky }
}
```

**Step 2: Update hooks/index.ts**

```typescript
export { useTheme } from './useTheme'
export { useStreamingText } from './useStreamingText'
export { useSearch } from './useSearch'
export { useScrollComposer } from './useScrollComposer'
```

**Step 3: Commit**

```bash
git add src/hooks/useScrollComposer.ts src/hooks/index.ts
git commit -m "feat: add useScrollComposer hook for sticky trigger"
```

---

## Task 3: MinimalHeader Component

**Files:**
- Create: `src/features/chat-layout/MinimalHeader.tsx`

**Step 1: Create MinimalHeader**

```tsx
import { Sun, Moon } from 'lucide-react'
import { useTheme } from '@/hooks'

export function MinimalHeader() {
  const { mode, toggleMode } = useTheme()

  return (
    <header className="px-4 h-14 flex items-center justify-between">
      <button
        onClick={() => window.location.reload()}
        className="text-xl font-semibold text-[var(--accent)] hover:opacity-80 transition-opacity"
      >
        WebSearch
      </button>

      <button
        onClick={toggleMode}
        className="p-2 rounded-lg hover:bg-[var(--bg-tertiary)] text-[var(--text-secondary)]"
      >
        {mode === 'dark' ? <Sun className="w-5 h-5" /> : <Moon className="w-5 h-5" />}
      </button>
    </header>
  )
}
```

**Step 2: Commit**

```bash
git add src/features/chat-layout/MinimalHeader.tsx
git commit -m "feat: add MinimalHeader for chat layout"
```

---

## Task 4: ChatHomePage

**Files:**
- Create: `src/features/chat-layout/ChatHomePage.tsx`

**Step 1: Create ChatHomePage**

```tsx
import { SearchComposer } from '@/components/composed/SearchComposer'
import { MinimalHeader } from './MinimalHeader'
import { Chip } from '@/components/ui'
import { Sparkles, Code, FileText } from 'lucide-react'

type SearchMode = 'search' | 'deepThink'

interface ChatHomePageProps {
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  mode: SearchMode
  onModeChange: (mode: SearchMode) => void
}

const QUICK_ACTIONS = [
  { label: 'AI Overview', icon: Sparkles, query: 'What is artificial intelligence?' },
  { label: 'Write Code', icon: Code, query: 'How do I write a React hook?' },
  { label: 'Summarize', icon: FileText, query: 'Summarize the latest tech news' },
]

export function ChatHomePage({
  value,
  onChange,
  onSubmit,
  mode,
  onModeChange,
}: ChatHomePageProps) {
  const handleQuickAction = (query: string) => {
    onChange(query)
    // Small delay to show the query, then submit
    setTimeout(() => onSubmit(), 100)
  }

  return (
    <div className="min-h-screen flex flex-col">
      <MinimalHeader />

      <main className="flex-1 flex flex-col items-center justify-center px-4 pb-32">
        <h1 className="text-3xl md:text-4xl font-semibold text-[var(--text-primary)] mb-8 text-center">
          Hva vil du vite?
        </h1>

        <SearchComposer
          variant="hero"
          value={value}
          onChange={onChange}
          onSubmit={onSubmit}
          mode={mode}
          onModeChange={onModeChange}
          autoFocus
        />

        {/* Quick actions */}
        <div className="flex flex-wrap justify-center gap-2 mt-6">
          {QUICK_ACTIONS.map(({ label, icon: Icon, query }) => (
            <Chip key={label} onClick={() => handleQuickAction(query)}>
              <Icon className="w-4 h-4 mr-1.5" />
              {label}
            </Chip>
          ))}
        </div>

        <p className="text-sm text-[var(--text-muted)] mt-8">
          Trykk <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-tertiary)] font-mono text-xs">Enter</kbd> for å søke
        </p>
      </main>
    </div>
  )
}
```

**Step 2: Commit**

```bash
git add src/features/chat-layout/ChatHomePage.tsx
git commit -m "feat: add ChatHomePage with hero composer"
```

---

## Task 5: ChatResultsPage

**Files:**
- Create: `src/features/chat-layout/ChatResultsPage.tsx`

**Step 1: Create ChatResultsPage**

```tsx
import { SearchComposer } from '@/components/composed/SearchComposer'
import { MinimalHeader } from './MinimalHeader'
import { useScrollComposer } from '@/hooks'
import { ResultsLayout } from '@/features/search/ResultsLayout'
import { ResultsList } from '@/features/search/ResultsList'
import { DiscussionsList } from '@/features/search/DiscussionsList'
import { AnswerPanel } from '@/features/search/AnswerPanel'
import type { SearchResult, AnswerModel, Intent } from '@/lib/types'

type SearchMode = 'search' | 'deepThink'

interface ChatResultsPageProps {
  query: string
  effectiveIntent: Intent
  results: SearchResult[] | null
  discussions: SearchResult[] | null
  answer: AnswerModel | null
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  mode: SearchMode
  onModeChange: (mode: SearchMode) => void
}

export function ChatResultsPage({
  query,
  effectiveIntent,
  results,
  discussions,
  answer,
  value,
  onChange,
  onSubmit,
  mode,
  onModeChange,
}: ChatResultsPageProps) {
  const { composerRef, showSticky } = useScrollComposer()

  const serpVariant = effectiveIntent === 'search' ? 'full' : 'compact'
  const answerVariant = effectiveIntent === 'chat' ? 'full' : 'compact'

  return (
    <div className="min-h-screen flex flex-col">
      {/* Header with optional sticky composer */}
      <header className="sticky top-0 z-20 bg-[var(--bg-primary)] border-b border-[var(--border)]">
        <div className="max-w-6xl mx-auto px-4 h-14 flex items-center gap-4">
          <button
            onClick={() => window.location.reload()}
            className="text-xl font-semibold text-[var(--accent)] hover:opacity-80 transition-opacity shrink-0"
          >
            WebSearch
          </button>

          {/* Sticky composer - only visible when scrolled */}
          {showSticky && (
            <SearchComposer
              variant="sticky"
              value={value}
              onChange={onChange}
              onSubmit={onSubmit}
              mode={mode}
              onModeChange={onModeChange}
            />
          )}
        </div>
      </header>

      {/* Content */}
      <main className="flex-1 px-4 py-6">
        <div className="max-w-6xl mx-auto">
          {/* Inline composer - measured for scroll trigger */}
          <div ref={composerRef} className="mb-6">
            <SearchComposer
              variant="inline"
              value={value}
              onChange={onChange}
              onSubmit={onSubmit}
              mode={mode}
              onModeChange={onModeChange}
            />
          </div>

          {/* Results */}
          <ResultsLayout
            intent={effectiveIntent}
            answerPanel={
              answer && <AnswerPanel answer={answer} variant={answerVariant} />
            }
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

**Step 2: Commit**

```bash
git add src/features/chat-layout/ChatResultsPage.tsx
git commit -m "feat: add ChatResultsPage with inline and sticky composer"
```

---

## Task 6: ChatLoadingPage

**Files:**
- Create: `src/features/chat-layout/ChatLoadingPage.tsx`

**Step 1: Create ChatLoadingPage**

```tsx
import type { LoadingPhase } from '@/lib/types'
import { MinimalHeader } from './MinimalHeader'
import { LoadingSpinner, SkeletonResults } from '@/features/loading'

interface ChatLoadingPageProps {
  query: string
  phase: LoadingPhase | null
}

const PHASE_LABELS: Record<LoadingPhase, string> = {
  understanding: 'Forstår spørsmålet...',
  searching: 'Søker i kilder...',
  synthesizing: 'Lager svar...',
}

export function ChatLoadingPage({ query, phase }: ChatLoadingPageProps) {
  return (
    <div className="min-h-screen flex flex-col">
      <MinimalHeader />

      {/* Query display */}
      <div className="px-4 py-4 border-b border-[var(--border)]">
        <div className="max-w-3xl mx-auto">
          <div className="px-4 py-3 rounded-xl bg-[var(--bg-secondary)] border border-[var(--border)]">
            <p className="text-[var(--text-primary)]">{query}</p>
          </div>
        </div>
      </div>

      {/* Loading content */}
      <main className="flex-1 flex flex-col items-center justify-center gap-6 p-8">
        <LoadingSpinner />
        <p className="text-lg text-[var(--text-secondary)]">
          {phase ? PHASE_LABELS[phase] : 'Laster...'}
        </p>
        <SkeletonResults />
      </main>
    </div>
  )
}
```

**Step 2: Commit**

```bash
git add src/features/chat-layout/ChatLoadingPage.tsx
git commit -m "feat: add ChatLoadingPage"
```

---

## Task 7: ChatLayout Wrapper

**Files:**
- Create: `src/features/chat-layout/ChatLayout.tsx`
- Create: `src/features/chat-layout/index.ts`

**Step 1: Create ChatLayout**

```tsx
import { useState, useEffect } from 'react'
import { ChatHomePage } from './ChatHomePage'
import { ChatLoadingPage } from './ChatLoadingPage'
import { ChatResultsPage } from './ChatResultsPage'
import type { useSearch } from '@/hooks'

type SearchMode = 'search' | 'deepThink'

interface ChatLayoutProps {
  search: ReturnType<typeof useSearch>
}

export function ChatLayout({ search }: ChatLayoutProps) {
  const [mode, setMode] = useState<SearchMode>('search')
  const [inputValue, setInputValue] = useState('')

  const handleSubmit = () => {
    if (inputValue.trim()) {
      search.search(inputValue.trim())
    }
  }

  // Sync input with current query when on results page
  useEffect(() => {
    if (search.view === 'results' && search.query) {
      setInputValue(search.query)
    }
  }, [search.view, search.query])

  // Clear input when going back to home
  useEffect(() => {
    if (search.view === 'home') {
      setInputValue('')
    }
  }, [search.view])

  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)]">
      {search.view === 'home' && (
        <ChatHomePage
          value={inputValue}
          onChange={setInputValue}
          onSubmit={handleSubmit}
          mode={mode}
          onModeChange={setMode}
        />
      )}

      {search.view === 'loading' && (
        <ChatLoadingPage query={search.query} phase={search.phase} />
      )}

      {search.view === 'results' && (
        <ChatResultsPage
          query={search.query}
          effectiveIntent={search.effectiveIntent}
          results={search.results}
          discussions={search.discussions}
          answer={search.answer}
          value={inputValue}
          onChange={setInputValue}
          onSubmit={handleSubmit}
          mode={mode}
          onModeChange={setMode}
        />
      )}
    </div>
  )
}
```

**Step 2: Create index.ts**

```typescript
export { ChatLayout } from './ChatLayout'
export { ChatHomePage } from './ChatHomePage'
export { ChatResultsPage } from './ChatResultsPage'
export { ChatLoadingPage } from './ChatLoadingPage'
export { MinimalHeader } from './MinimalHeader'
```

**Step 3: Commit**

```bash
git add src/features/chat-layout/ChatLayout.tsx src/features/chat-layout/index.ts
git commit -m "feat: add ChatLayout wrapper with view coordination"
```

---

## Task 8: Refactor Classic Layout

**Files:**
- Create: `src/features/classic-layout/ClassicLayout.tsx`
- Create: `src/features/classic-layout/index.ts`

**Step 1: Create ClassicLayout**

```tsx
import { HomePage } from '@/features/home'
import { LoadingPage } from '@/features/loading'
import { SearchPage } from '@/features/search'
import type { useSearch } from '@/hooks'

interface ClassicLayoutProps {
  search: ReturnType<typeof useSearch>
}

export function ClassicLayout({ search }: ClassicLayoutProps) {
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

**Step 2: Create index.ts**

```typescript
export { ClassicLayout } from './ClassicLayout'
```

**Step 3: Commit**

```bash
git add src/features/classic-layout/ClassicLayout.tsx src/features/classic-layout/index.ts
git commit -m "refactor: extract ClassicLayout to separate feature"
```

---

## Task 9: Update App.tsx with Layout Switch

**Files:**
- Modify: `src/App.tsx`

**Step 1: Update App.tsx**

```tsx
import { useSearch, useTheme } from '@/hooks'
import { ClassicLayout } from '@/features/classic-layout'
import { ChatLayout } from '@/features/chat-layout'

// Layout mode - change this to switch between layouts
const LAYOUT_MODE: 'classic' | 'chat' = 'chat'

export default function App() {
  useTheme()
  const search = useSearch()

  if (LAYOUT_MODE === 'classic') {
    return <ClassicLayout search={search} />
  }

  return <ChatLayout search={search} />
}
```

**Step 2: Commit**

```bash
git add src/App.tsx
git commit -m "feat: add layout mode switch between classic and chat"
```

---

## Task 10: Update Composed Components Export

**Files:**
- Modify: `src/components/composed/index.ts`

**Step 1: Update index.ts**

```typescript
export { IntentToggle } from './IntentToggle'
export { SearchComposer } from './SearchComposer'
```

**Step 2: Commit**

```bash
git add src/components/composed/index.ts
git commit -m "chore: export SearchComposer from composed components"
```

---

## Task 11: Verify and Test

**Step 1: Check TypeScript**

Run: `npx tsc --noEmit`
Expected: No errors

**Step 2: Run dev server**

Run: `npm run dev`
Expected: Chat layout shows with hero composer

**Step 3: Test flow**

1. Type a query in hero composer
2. Press Enter - should show loading then results
3. Scroll down - sticky composer should appear
4. Type new query in sticky - should search again

**Step 4: Test classic layout**

Change `LAYOUT_MODE` to `'classic'` in App.tsx and verify it still works.

**Step 5: Final commit**

```bash
git add -A
git commit -m "chore: verify chat layout implementation"
```

---

## Summary

| Task | Component | Beskrivelse |
|------|-----------|-------------|
| 1 | SearchComposer | Kjerne-komponent med 3 varianter |
| 2 | useScrollComposer | Hook for sticky-trigger |
| 3 | MinimalHeader | Enkel header for chat layout |
| 4 | ChatHomePage | Hero-visning med sentrert composer |
| 5 | ChatResultsPage | Results med inline + sticky |
| 6 | ChatLoadingPage | Loading med query display |
| 7 | ChatLayout | Wrapper med view-koordinering |
| 8 | ClassicLayout | Refaktorert classic til egen feature |
| 9 | App.tsx | Layout mode switch |
| 10 | Exports | Oppdaterte barrel exports |
| 11 | Verify | TypeScript + manuell test |
