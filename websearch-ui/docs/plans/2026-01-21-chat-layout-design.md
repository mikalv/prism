# Chat Layout Design (Z.ai-stil)

> Alternativ layout med hero→results→sticky transformasjon, inspirert av Z.ai/Brave.

**Dato:** 2026-01-21
**Status:** Design ferdig
**Scope:** Alternativ layout ved siden av "classic"

---

## 1. Konsept

To layouts som kan byttes på kode-nivå:
- **Classic**: Dagens oppsett med topbar-input fra start
- **Chat**: Z.ai-stil med hero-composer som transformerer til sticky ved scroll

Formål: Eksperimentere med hva som fungerer best når backend er klar.

---

## 2. Layout-tilstander

```
┌─────────────────────────────────────────────────────────┐
│ 1. HOME                                                 │
│                                                         │
│    ┌─────────────────────────────────────┐              │
│    │  Minimal header (logo + theme)      │              │
│    └─────────────────────────────────────┘              │
│                                                         │
│                      ↓ whitespace                       │
│                                                         │
│         ┌─────────────────────────────┐                 │
│         │   SearchComposer (hero)     │                 │
│         │   multiline, stor, rounded  │                 │
│         └─────────────────────────────┘                 │
│              [Quick actions chips]                      │
│                                                         │
└─────────────────────────────────────────────────────────┘
                        │
                        │ onSubmit
                        ▼
┌─────────────────────────────────────────────────────────┐
│ 2. RESULTS (scrollY = 0)                                │
│                                                         │
│    ┌─────────────────────────────────────┐              │
│    │  Minimal header (logo)              │  ← sticky    │
│    └─────────────────────────────────────┘              │
│                                                         │
│         ┌─────────────────────────────┐                 │
│         │  SearchComposer (inline)    │  ← i content   │
│         │  query vises, kan redigeres │    (ref måles) │
│         └─────────────────────────────┘                 │
│                                                         │
│    ┌──────────────────┐  ┌──────────────────┐          │
│    │   AnswerPanel    │  │    SERP          │          │
│    └──────────────────┘  └──────────────────┘          │
│                                                         │
└─────────────────────────────────────────────────────────┘
                        │
                        │ scroll ned
                        ▼
┌─────────────────────────────────────────────────────────┐
│ 3. RESULTS (scrolled)                                   │
│                                                         │
│    ┌─────────────────────────────────────┐              │
│    │  Header + SearchComposer (sticky)   │  ← kompakt  │
│    └─────────────────────────────────────┘              │
│                                                         │
│    ┌──────────────────┐  ┌──────────────────┐          │
│    │   AnswerPanel    │  │    SERP          │          │
│    └──────────────────┘  └──────────────────┘          │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## 3. SearchComposer-komponenten

### Struktur (Z.ai-inspirert)

```
┌─────────────────────────────────────────────────────────┐
│                                                         │
│   textarea (auto-resize, 1-6 linjer)                   │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ [+]  [🔍 Search] [🌐 Deep Think]                   [→] │
└─────────────────────────────────────────────────────────┘
```

### Varianter

| Variant | Textarea | Knapper | Quick Actions |
|---------|----------|---------|---------------|
| `hero` | multiline, stor, min-h-72px | alle synlige | ja, under |
| `inline` | multiline, medium, min-h-56px | alle synlige | nei |
| `sticky` | én-linje, h-40px | kun send | nei |

### Props

```typescript
interface SearchComposerProps {
  variant: 'hero' | 'inline' | 'sticky'
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  mode: 'search' | 'deepThink'
  onModeChange: (mode: 'search' | 'deepThink') => void
  disabled?: boolean
  showQuickActions?: boolean
  autoFocus?: boolean
}
```

### Keyboard

- **Enter**: Submit (hvis ikke tom)
- **Shift+Enter**: Ny linje
- **/**: Fokuser input (global hotkey)

### Styling (fra Z.ai)

**Wrapper:**
```css
rounded-xl
border border-white/10
bg-[var(--bg-secondary)]
shadow-[0px_4px_16px_0px_rgba(0,0,0,0.1)]
```

**Mode pills:**
```css
rounded-lg
border border-white/10
px-2 py-1.5 text-sm
hover:bg-[var(--bg-tertiary)]

/* Aktiv: */
bg-[var(--accent)]/10
border-[var(--accent)]/20
text-[var(--accent)]
```

**Send-knapp:**
```css
rounded-lg p-2
disabled: bg-[var(--bg-tertiary)] text-[var(--text-muted)]
enabled: bg-[var(--accent)] text-white
```

---

## 4. Scroll-logikk

IntersectionObserver på inline-composer:

```typescript
export function useScrollComposer() {
  const composerRef = useRef<HTMLDivElement>(null)
  const [showSticky, setShowSticky] = useState(false)

  useEffect(() => {
    const el = composerRef.current
    if (!el) return

    const observer = new IntersectionObserver(
      ([entry]) => setShowSticky(!entry.isIntersecting),
      { threshold: 0, rootMargin: '-60px 0px 0px 0px' }
    )

    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  return { composerRef, showSticky }
}
```

Sticky composer vises når inline-composer scroller ut av viewport.

---

## 5. Filstruktur

**Nye filer:**

```
src/
├── components/
│   └── composed/
│       └── SearchComposer.tsx      # Kjerne-komponenten
│
├── features/
│   └── chat-layout/
│       ├── index.ts
│       ├── ChatLayout.tsx          # Wrapper
│       ├── ChatHomePage.tsx        # Hero-visning
│       ├── ChatResultsPage.tsx     # Results + sticky
│       └── MinimalHeader.tsx       # Logo + theme toggle
│
└── hooks/
    └── useScrollComposer.ts        # IntersectionObserver
```

**Gjenbrukes fra classic:**
- `features/search/AnswerPanel.tsx`
- `features/search/ResultsList.tsx`
- `features/search/ResultsLayout.tsx`
- `features/loading/*`
- `hooks/useSearch.ts`
- `hooks/useStreamingText.ts`
- `lib/*`

---

## 6. Layout-bytte

I `App.tsx`:

```typescript
const LAYOUT_MODE = 'chat' as 'classic' | 'chat'

export default function App() {
  useTheme()
  const search = useSearch()

  if (LAYOUT_MODE === 'classic') {
    return <ClassicLayout search={search} />
  }

  return <ChatLayout search={search} />
}
```

---

## 7. Komponenter

### ChatLayout.tsx

```tsx
export function ChatLayout({ search }: ChatLayoutProps) {
  const [mode, setMode] = useState<'search' | 'deepThink'>('search')
  const [inputValue, setInputValue] = useState('')

  const handleSubmit = () => {
    if (inputValue.trim()) {
      search.search(inputValue.trim())
    }
  }

  return (
    <div className="min-h-screen bg-[var(--bg-primary)]">
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
          search={search}
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

### ChatHomePage.tsx

```tsx
export function ChatHomePage({ value, onChange, onSubmit, mode, onModeChange }) {
  return (
    <div className="min-h-screen flex flex-col">
      <MinimalHeader />

      <main className="flex-1 flex flex-col items-center justify-center px-4 pb-32">
        <h1 className="text-3xl font-semibold mb-8">
          Hva vil du vite?
        </h1>

        <SearchComposer
          variant="hero"
          value={value}
          onChange={onChange}
          onSubmit={onSubmit}
          mode={mode}
          onModeChange={onModeChange}
          showQuickActions
          autoFocus
        />

        <QuickActions className="mt-6" />
      </main>
    </div>
  )
}
```

### ChatResultsPage.tsx

```tsx
export function ChatResultsPage({ search, value, onChange, onSubmit, mode, onModeChange }) {
  const { composerRef, showSticky } = useScrollComposer()

  return (
    <div className="min-h-screen flex flex-col">
      <header className="sticky top-0 z-20 bg-[var(--bg-primary)] border-b border-[var(--border)]">
        <div className="max-w-6xl mx-auto px-4 h-14 flex items-center gap-4">
          <Logo />

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

      <main className="flex-1 px-4 py-6">
        <div className="max-w-6xl mx-auto">
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

          <ResultsLayout
            intent={search.effectiveIntent}
            answerPanel={search.answer && <AnswerPanel answer={search.answer} />}
            serpPanel={search.results && <ResultsList results={search.results} />}
          />
        </div>
      </main>
    </div>
  )
}
```

---

## 8. Neste steg

1. Implementer `SearchComposer` med alle varianter
2. Implementer `useScrollComposer` hook
3. Bygg `chat-layout/` feature
4. Refaktorer classic layout til `features/classic-layout/`
5. Test begge layouts
