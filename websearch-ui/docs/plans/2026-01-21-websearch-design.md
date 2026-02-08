# WebSearch Frontend Design

> Et søkefelt som føles som en søkemotor, men som løfter resultatene til en strukturert LLM-svarflate.

**Dato:** 2026-01-21
**Status:** Godkjent for implementering
**Scope:** Kjerne MVP (Fase 1)

---

## 1. Produktoversikt

### Konsept
Hybrid søke-/chat-interface inspirert av Perplexity og z.ai. Brukeren skriver en query og får både tradisjonelle søkeresultater (SERP) og et LLM-generert svar - alltid begge, men med adaptiv vekting basert på query-type.

### Nøkkelegenskaper
- **SPA-transformasjon:** Home → Loading → Results uten page reload
- **Intent-klassifisering:** Automatisk gjenkjenning av spørsmål vs. keyword-søk
- **Adaptive layout:** Begge paneler synlige, vekting tilpasses intent
- **Premium feel:** 3-fase loading, streaming-simulering, smooth animasjoner
- **Tema-system:** 2 temaer × 2 modes = 4 kombinasjoner

---

## 2. Tech Stack

| Kategori | Valg | Begrunnelse |
|----------|------|-------------|
| Framework | React 18+ | Kjent, stort økosystem |
| Bundler | Vite | Rask dev server, enkel config |
| Styling | Tailwind CSS v4 | Utility-first, CSS variables, JIT |
| Routing | Ingen (SPA state) | URL oppdateres med history.replaceState |
| UI Primitives | Radix UI | Headless, tilgjengelig, ingen styling |
| Ikoner | Lucide React | Lett, konsistent, tree-shakeable |
| Språk | TypeScript | Type-sikkerhet |

### Dependencies
```json
{
  "dependencies": {
    "react": "^18.2.0",
    "react-dom": "^18.2.0",
    "@radix-ui/react-popover": "^1.0.0",
    "@radix-ui/react-switch": "^1.0.0",
    "lucide-react": "^0.300.0"
  },
  "devDependencies": {
    "vite": "^5.0.0",
    "@vitejs/plugin-react": "^4.0.0",
    "tailwindcss": "^4.0.0",
    "typescript": "^5.0.0"
  }
}
```

---

## 3. Prosjektstruktur

```
websearch/
├── index.html
├── package.json
├── vite.config.ts
├── tailwind.config.ts
├── tsconfig.json
│
├── docs/
│   └── plans/
│       └── 2026-01-21-websearch-design.md
│
├── src/
│   ├── main.tsx                    # Entry point, render App
│   ├── App.tsx                     # SearchContainer wrapper
│   ├── index.css                   # Tailwind + tema CSS-variabler
│   │
│   ├── components/
│   │   ├── ui/                     # Primitive building blocks
│   │   │   ├── Button.tsx
│   │   │   ├── Input.tsx
│   │   │   ├── Card.tsx
│   │   │   ├── Chip.tsx
│   │   │   ├── Popover.tsx         # Radix wrapper
│   │   │   ├── Switch.tsx          # Radix wrapper
│   │   │   └── Skeleton.tsx
│   │   │
│   │   └── composed/               # Sammensatte komponenter
│   │       ├── SearchModePopover.tsx
│   │       ├── IntentToggle.tsx
│   │       └── ThemeSelector.tsx
│   │
│   ├── features/
│   │   ├── home/
│   │   │   ├── HomePage.tsx        # Container for home view
│   │   │   ├── SearchHero.tsx      # Tittel + stor input
│   │   │   └── QuickActions.tsx    # Chips under input
│   │   │
│   │   ├── search/
│   │   │   ├── SearchPage.tsx      # Container for results view
│   │   │   ├── TopBar.tsx          # Sticky header
│   │   │   ├── ResultsLayout.tsx   # Adaptive 2-kolonne grid
│   │   │   ├── ResultsList.tsx     # SERP kolonne
│   │   │   ├── ResultCard.tsx      # Enkelt søkeresultat
│   │   │   ├── DiscussionsList.tsx # Reddit/SO/GitHub
│   │   │   ├── AnswerPanel.tsx     # LLM-svar kolonne
│   │   │   └── CitationBadge.tsx   # Kildehenvisning
│   │   │
│   │   └── loading/
│   │       ├── LoadingPage.tsx     # 3-fase loading view
│   │       ├── LoadingSpinner.tsx  # Animerte dots
│   │       └── SkeletonResults.tsx # Placeholder cards
│   │
│   ├── hooks/
│   │   ├── useSearch.ts            # Hovedlogikk, state machine
│   │   ├── useTheme.ts             # Tema + dark mode
│   │   └── useStreamingText.ts     # Fake streaming effect
│   │
│   └── lib/
│       ├── types.ts                # TypeScript interfaces
│       ├── intent.ts               # Query classifier
│       ├── mock-data.ts            # Hardkodet testdata
│       └── themes.ts               # Tema-definisjoner
│
└── TODOs.md                        # Full roadmap
```

---

## 4. Tema-system

### CSS-variabler (index.css)

```css
:root {
  /* Layout */
  --radius-sm: 8px;
  --radius-md: 12px;
  --radius-lg: 16px;
  --radius-xl: 24px;

  /* Shadows */
  --shadow-sm: 0 1px 2px rgba(0,0,0,0.05);
  --shadow-md: 0 4px 6px rgba(0,0,0,0.07);
  --shadow-lg: 0 10px 15px rgba(0,0,0,0.1);
  --shadow-xl: 0 20px 25px rgba(0,0,0,0.15);
}

/* Tema 1: Neutral (z.ai-inspirert) */
.theme-neutral {
  --accent: #6b7280;
  --accent-hover: #4b5563;
  --accent-subtle: #9ca3af;
}

.theme-neutral.light {
  --bg-primary: #F4F6F8;
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
  --accent: #20B2AA;
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
```

### useTheme Hook

```typescript
interface ThemeState {
  theme: 'neutral' | 'teal'
  mode: 'light' | 'dark'
}

function useTheme() {
  const [state, setState] = useState<ThemeState>(() => {
    const saved = localStorage.getItem('theme')
    return saved ? JSON.parse(saved) : { theme: 'teal', mode: 'dark' }
  })

  useEffect(() => {
    const html = document.documentElement
    html.className = `theme-${state.theme} ${state.mode}`
    localStorage.setItem('theme', JSON.stringify(state))
  }, [state])

  return {
    ...state,
    setTheme: (theme) => setState(s => ({ ...s, theme })),
    setMode: (mode) => setState(s => ({ ...s, mode })),
    toggleMode: () => setState(s => ({
      ...s,
      mode: s.mode === 'light' ? 'dark' : 'light'
    }))
  }
}
```

---

## 5. App-arkitektur

### State Machine (useSearch)

```
        ┌─────────┐
        │  home   │
        └────┬────┘
             │ search(query)
             ▼
        ┌─────────┐
        │ loading │ ← phase: understanding → searching → synthesizing
        └────┬────┘
             │ data ready
             ▼
        ┌─────────┐
        │ results │
        └────┬────┘
             │ new search OR reset
             ▼
        (back to loading or home)
```

### SearchState Interface

```typescript
interface SearchState {
  view: 'home' | 'loading' | 'results'
  query: string
  intent: 'chat' | 'search'
  intentOverride: 'chat' | 'search' | null  // User override
  phase: 'understanding' | 'searching' | 'synthesizing' | null
  results: SearchResult[] | null
  discussions: SearchResult[] | null
  answer: AnswerModel | null
}
```

### App.tsx Struktur

```tsx
function App() {
  return (
    <ThemeProvider>
      <SearchContainer />
    </ThemeProvider>
  )
}

function SearchContainer() {
  const search = useSearch()

  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--text-primary)]">
      {search.view === 'home' && (
        <HomePage onSearch={search.search} />
      )}
      {search.view === 'loading' && (
        <LoadingPage query={search.query} phase={search.phase} />
      )}
      {search.view === 'results' && (
        <SearchPage {...search} onNewSearch={search.search} />
      )}
    </div>
  )
}
```

---

## 6. Intent-klassifisering

### Heuristikk

```typescript
export type Intent = 'chat' | 'search'

const QUESTION_STARTERS = [
  // Norsk
  'hva', 'hvordan', 'hvorfor', 'hvem', 'hvor', 'når', 'hvilken',
  'kan du', 'forklar', 'sammenlign', 'hjelp', 'anbefal',
  // Engelsk
  'what', 'how', 'why', 'who', 'where', 'when', 'which',
  'explain', 'compare', 'help', 'recommend', 'tell me'
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
  if (QUESTION_STARTERS.some(s => q.startsWith(s + ' ') || q === s)) {
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

---

## 7. Adaptive Layout

### Vekting basert på intent

| Intent | Venstre kolonne | Høyre kolonne |
|--------|-----------------|---------------|
| `chat` | AnswerPanel (65%) | SERP (35%) |
| `search` | SERP (60%) | AnswerPanel (40%) |

### ResultsLayout.tsx

```tsx
function ResultsLayout({
  intent,
  answerPanel,
  serpPanel
}: ResultsLayoutProps) {
  const isChat = intent === 'chat'

  return (
    <div className={`
      grid gap-6 transition-all duration-300
      ${isChat
        ? 'grid-cols-[65fr_35fr]'
        : 'grid-cols-[60fr_40fr]'}
    `}>
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

### Komponent-varianter

AnswerPanel og ResultsList tar en `variant` prop:

```tsx
<AnswerPanel variant={intent === 'chat' ? 'full' : 'compact'} />
<ResultsList variant={intent === 'search' ? 'full' : 'compact'} />
```

**Forskjeller:**

| Komponent | `full` | `compact` |
|-----------|--------|-----------|
| AnswerPanel | Alle keyPoints, store follow-ups | 3-4 keyPoints, mindre |
| ResultsList | Fulle snippets, discussions | Kortere snippets, ingen discussions |

---

## 8. Loading States

### 3 Faser

```typescript
const LOADING_PHASES = [
  { key: 'understanding', label: 'Understanding query...', duration: 300 },
  { key: 'searching', label: 'Searching sources...', duration: 1000 },
  { key: 'synthesizing', label: 'Synthesizing answer...', duration: 200 }
]
```

### LoadingPage Layout

```
┌──────────────────────────────────────────────────────────┐
│ TopBar: [Logo] [query, disabled] [ThemeToggle]           │
├──────────────────────────────────────────────────────────┤
│                                                          │
│                    ●  ●  ●                               │
│              Understanding query...                      │
│                                                          │
│    ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│    │ ░░░░░░░░░░░ │  │ ░░░░░░░░░░░ │  │ ░░░░░░░░░░░ │    │
│    │ ░░░░░░░     │  │ ░░░░░░░     │  │ ░░░░░░░     │    │
│    │ ░░░░░░░░░░░ │  │ ░░░░░░░░░░░ │  │ ░░░░░░░░░░░ │    │
│    └─────────────┘  └─────────────┘  └─────────────┘    │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### Animasjoner

```css
/* Dot spinner */
@keyframes pulse-dot {
  0%, 80%, 100% { opacity: 0.3; transform: scale(0.8); }
  40% { opacity: 1; transform: scale(1); }
}

.dot-spinner span:nth-child(1) { animation-delay: 0ms; }
.dot-spinner span:nth-child(2) { animation-delay: 150ms; }
.dot-spinner span:nth-child(3) { animation-delay: 300ms; }

/* Skeleton pulse */
.skeleton {
  @apply animate-pulse bg-[var(--bg-tertiary)] rounded;
}

/* Fade in up */
@keyframes fade-in-up {
  from { opacity: 0; transform: translateY(8px); }
  to { opacity: 1; transform: translateY(0); }
}

.animate-fade-in-up {
  animation: fade-in-up 0.3s ease-out forwards;
}
```

---

## 9. Streaming-simulering

### useStreamingText Hook

```typescript
function useStreamingText(
  fullText: string,
  options?: { speed?: number; delay?: number }
) {
  const { speed = 20, delay = 0 } = options ?? {}
  const [displayed, setDisplayed] = useState('')
  const [isComplete, setIsComplete] = useState(false)

  useEffect(() => {
    setDisplayed('')
    setIsComplete(false)

    const timeout = setTimeout(() => {
      let i = 0
      const interval = setInterval(() => {
        if (i <= fullText.length) {
          setDisplayed(fullText.slice(0, i))
          i++
        } else {
          setIsComplete(true)
          clearInterval(interval)
        }
      }, speed)

      return () => clearInterval(interval)
    }, delay)

    return () => clearTimeout(timeout)
  }, [fullText, speed, delay])

  return { displayed, isComplete }
}
```

### Bruk i AnswerPanel

```tsx
function AnswerPanel({ answer }: { answer: AnswerModel }) {
  const { displayed: shortAnswer, isComplete } = useStreamingText(
    answer.shortAnswer,
    { delay: 200 }
  )

  return (
    <div>
      <p className="text-lg">{shortAnswer}</p>
      {isComplete && (
        <ul className="animate-fade-in-up">
          {answer.keyPoints.map((point, i) => (
            <KeyPoint key={i} text={point} delay={i * 100} />
          ))}
        </ul>
      )}
    </div>
  )
}
```

---

## 10. Popover-design

### SearchModePopover

```tsx
import * as Popover from '@radix-ui/react-popover'
import * as Switch from '@radix-ui/react-switch'

function SearchModePopover({
  deepThink,
  onDeepThinkChange
}: SearchModePopoverProps) {
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <button className="p-2 rounded-full hover:bg-[var(--bg-tertiary)]">
          <Info className="w-4 h-4" />
        </button>
      </Popover.Trigger>

      <Popover.Portal>
        <Popover.Content
          side="bottom"
          align="start"
          sideOffset={8}
          className="
            w-80 p-4 rounded-2xl
            bg-black/90 backdrop-blur-md
            border border-white/10
            shadow-xl text-white text-sm
            animate-fade-in-up
          "
        >
          <div className="space-y-3">
            <div>
              <div className="font-semibold">Search</div>
              <div className="text-white/70">
                Single-round search, quickly get information
              </div>
            </div>

            <div className="h-px bg-white/10" />

            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="font-semibold">Deep Think</div>
                <div className="text-white/70">
                  Multi-round search, in-depth research
                </div>
              </div>
              <Switch.Root
                checked={deepThink}
                onCheckedChange={onDeepThinkChange}
                className="
                  w-10 h-6 rounded-full bg-white/20
                  data-[state=checked]:bg-[var(--accent)]
                "
              >
                <Switch.Thumb className="
                  block w-4 h-4 rounded-full bg-white
                  translate-x-1 transition-transform
                  data-[state=checked]:translate-x-5
                " />
              </Switch.Root>
            </div>
          </div>

          <Popover.Arrow className="fill-black/90" />
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}
```

---

## 11. Datatyper

```typescript
// lib/types.ts

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
  citations: Record<number, string[]>  // keyPoint index → source IDs
  followUps: string[]
  caveats: string[]
  confidence: 'low' | 'medium' | 'high'
}

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

export type Intent = 'chat' | 'search'
export type LoadingPhase = 'understanding' | 'searching' | 'synthesizing'
```

---

## 12. Komponenter - Oversikt

### Primitives (ui/)

| Komponent | Beskrivelse |
|-----------|-------------|
| `Button` | Primær, sekundær, ghost varianter |
| `Input` | Søkefelt med ikon-støtte |
| `Card` | Container med border, shadow |
| `Chip` | Klikkbar tag/badge |
| `Popover` | Radix wrapper med styling |
| `Switch` | Toggle switch |
| `Skeleton` | Loading placeholder |

### Composed (composed/)

| Komponent | Beskrivelse |
|-----------|-------------|
| `SearchModePopover` | Search/Deep Think info + toggle |
| `IntentToggle` | Answer-first / Search-first switch |
| `ThemeSelector` | Tema + dark mode velger |

### Features

| Feature | Komponenter |
|---------|-------------|
| `home` | HomePage, SearchHero, QuickActions |
| `search` | SearchPage, TopBar, ResultsLayout, ResultsList, ResultCard, AnswerPanel, CitationBadge |
| `loading` | LoadingPage, LoadingSpinner, SkeletonResults |

---

## 13. Neste steg

1. **Sett opp prosjekt:** Vite + React + TypeScript + Tailwind
2. **Implementer tema-system:** CSS-variabler + useTheme
3. **Bygg primitives:** Button, Input, Card, Chip, Skeleton
4. **Bygg Home:** SearchHero, QuickActions
5. **Bygg Loading:** LoadingPage med faser og skeletons
6. **Bygg Results:** ResultsLayout, ResultsList, AnswerPanel
7. **Koble sammen:** useSearch hook, SPA-transformasjon
8. **Polish:** Animasjoner, streaming, popover

---

## Backend API-krav (for fremtidig implementering)

### POST /api/search

**Request:**
```json
{
  "query": "string",
  "mode": "search" | "deep_think",
  "options": {
    "tone": "short" | "normal" | "deep",
    "citationLevel": "strict" | "light"
  }
}
```

**Response:**
```json
{
  "results": "SearchResult[]",
  "discussions": "SearchResult[]",
  "answer": "AnswerModel"
}
```

### GET /api/search/stream

Server-Sent Events for streaming LLM-svar. Events:
- `outline` - Initial struktur
- `content` - Inkrementell tekst
- `sources` - Kilder når klare
- `done` - Ferdig
