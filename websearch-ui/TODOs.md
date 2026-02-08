# WebSearch - Full Visjon & TODOs

## Produktidé
Et søkefelt som føles som en søkemotor, men som alltid kan "løfte" resultatene til en strukturert LLM-svarflate (med kilder, kort, oppsummering, og en "Deep Think"-modus som kjører en orkestrert pipeline med verifikasjon).

**Design:** Se `docs/plans/2026-01-21-websearch-design.md` for fullstendig design.

---

## Fase 1: Kjerne (MVP)
> Implementeres først. Home + Loading + Results med adaptiv layout.

### 1.1 Prosjekt-setup
- [ ] Vite + React + TypeScript
- [ ] Tailwind CSS v4
- [ ] Radix UI (popover, switch)
- [ ] Lucide React (ikoner)
- [ ] Mappestruktur: features/, components/ui/, components/composed/, hooks/, lib/

### 1.2 Tema-system
- [ ] CSS-variabler i index.css
- [ ] Tema 1: Neutral (z.ai-inspirert) - light/dark
- [ ] Tema 2: Teal (Perplexity-inspirert) - light/dark
- [ ] useTheme hook med localStorage persistering
- [ ] ThemeSelector komponent

### 1.3 UI Primitives (components/ui/)
- [ ] Button (primær, sekundær, ghost)
- [ ] Input (med ikon-støtte venstre/høyre)
- [ ] Card (border, shadow, hover)
- [ ] Chip (klikkbar tag)
- [ ] Skeleton (pulserende placeholder)
- [ ] Popover (Radix wrapper)
- [ ] Switch (Radix wrapper)

### 1.4 Home-side (features/home/)
- [ ] HomePage container
- [ ] SearchHero - tittel + stor input
- [ ] Modus-toggle (Search / Deep Think med popover)
- [ ] QuickActions - chips (AI Slides, Write Code, etc.)
- [ ] Hotkey: `/` fokuserer input
- [ ] Auto-focus på load

### 1.5 Loading-side (features/loading/)
- [ ] LoadingPage container
- [ ] 3-fase visning: Understanding → Searching → Synthesizing
- [ ] LoadingSpinner (3 dots med staggered animation)
- [ ] SkeletonResults (placeholder cards)
- [ ] Fade-transition mellom faser

### 1.6 Results-side (features/search/)
- [ ] SearchPage container
- [ ] TopBar (sticky, logo, søkefelt, toggles, tema)
- [ ] ResultsLayout (adaptive grid basert på intent)
- [ ] Intent-klassifisering (chat vs search)
- [ ] IntentToggle (Answer-first / Search-first override)

### 1.7 SERP-kolonne
- [ ] ResultsList container (full/compact varianter)
- [ ] ResultCard (title, domain, snippet, favicon, hover)
- [ ] DiscussionsList (Reddit, SO, GitHub)
- [ ] Scroll med fade-edge effekt

### 1.8 Answer-kolonne
- [ ] AnswerPanel container (full/compact varianter)
- [ ] shortAnswer med streaming-effekt
- [ ] keyPoints liste med staggered fade-in
- [ ] Concept chips (klikkbare)
- [ ] CitationBadge (nummer som refererer til kilder)
- [ ] Follow-up forslag
- [ ] Confidence indikator

### 1.9 State & Hooks
- [ ] useSearch hook (state machine: home → loading → results)
- [ ] useStreamingText hook (fake streaming)
- [ ] URL-oppdatering med history.replaceState (for deling)

### 1.10 Mock Data
- [ ] SearchResult[] (8-10 realistiske resultater)
- [ ] AnswerModel (komplett med citations)
- [ ] Discussions[] (2-3 Reddit/SO tråder)
- [ ] Simulert delay for realistisk feel

---

## Fase 2: Utvidet
> Etter kjerne er ferdig.

### 2.1 Kilde-drawer
- [ ] Klikk på CitationBadge åpner drawer
- [ ] Viser: title, domain, relevante tekstutdrag
- [ ] "Open" knapp til original kilde
- [ ] Markering av hvilken bullet som brukte kilden
- [ ] Slide-in animasjon

### 2.2 Ekte streaming
- [ ] useStreamingText med ReadableStream støtte
- [ ] Progressiv visning: outline → bullets → sources
- [ ] Cursor/caret animasjon under streaming

### 2.3 Command Palette
- [ ] Ctrl+K / Cmd+K åpner palette
- [ ] Søk i actions, quick actions
- [ ] Keyboard navigation (arrows, enter, esc)
- [ ] Fuzzy matching

### 2.4 SearchModePopover polish
- [ ] Pil/arrow som peker mot trigger
- [ ] Smooth open/close animasjon
- [ ] Keyboard support (esc lukker)

---

## Fase 3: Deep Think
> Agent pipeline visning.

### 3.1 Deep Think View
- [ ] Rapport-format i answer-panel
- [ ] Seksjoner: Plan/Pipeline, Findings, Evidence, Caveats, Answer
- [ ] Collapsible seksjoner

### 3.2 Agent Cards
- [ ] Researcher (innhenting)
- [ ] Analyst (syntese)
- [ ] Critic (sanity check)
- [ ] Writer (final)
- [ ] Status-indikator per agent

### 3.3 Deep Think Loading
- [ ] Steg-for-steg progress
- [ ] Hvilken agent som jobber nå
- [ ] Tidsbruk per steg

### 3.4 Mock DeepThinkTrace
- [ ] steps[] med name, status, summary
- [ ] disagreements[] med claim, sourcesFor/Against
- [ ] openQuestions[]

---

## Fase 4: Polish & Extras

### 4.1 Mikrointeraksjoner
- [ ] TopBar shrink on scroll
- [ ] Smooth layout transitions (grid-kolonne animasjon)
- [ ] Hover/focus states på alle interaktive elementer
- [ ] Keyboard shortcuts display (tooltip)

### 4.2 History/Sessions
- [ ] Sidebar eller dropdown med tidligere søk
- [ ] Persist til localStorage
- [ ] Rask re-søk fra historikk

### 4.3 Settings Panel
- [ ] Tone-kontroll (kort/normal/dypt)
- [ ] Siteringsnivå (strengt/lett)
- [ ] Region/språk preferanse

### 4.4 Search-as-you-type
- [ ] Autocomplete forslag
- [ ] Debounced (300ms)
- [ ] Keyboard navigation i suggestions

### 4.5 Accessibility
- [ ] ARIA labels på alle interaktive elementer
- [ ] Focus-visible styling
- [ ] Screen reader testing
- [ ] Reduced motion support

---

## Datastrukturer

```typescript
interface SearchResult {
  id: string
  title: string
  url: string
  displayDomain: string
  snippet: string
  publishedAt?: string
  faviconUrl?: string
  score: number
}

interface AnswerModel {
  shortAnswer: string
  keyPoints: string[]
  concepts: string[]
  citations: Record<number, string[]>
  followUps: string[]
  caveats: string[]
  confidence: 'low' | 'medium' | 'high'
}

interface DeepThinkTrace {
  steps: {
    name: string
    status: 'pending' | 'running' | 'done' | 'error'
    summary: string
    sourcesUsedCount: number
    durationMs: number
  }[]
  disagreements: {
    claim: string
    sourcesFor: string[]
    sourcesAgainst: string[]
  }[]
  openQuestions: string[]
}

interface SearchState {
  view: 'home' | 'loading' | 'results'
  query: string
  intent: 'chat' | 'search'
  intentOverride: 'chat' | 'search' | null
  phase: 'understanding' | 'searching' | 'synthesizing' | null
  results: SearchResult[] | null
  discussions: SearchResult[] | null
  answer: AnswerModel | null
}
```

---

## Backend API-krav

### POST /api/search
```json
// Request
{
  "query": "string",
  "mode": "search" | "deep_think",
  "options": {
    "tone": "short" | "normal" | "deep",
    "citationLevel": "strict" | "light"
  }
}

// Response
{
  "results": "SearchResult[]",
  "discussions": "SearchResult[]",
  "answer": "AnswerModel",
  "trace"?: "DeepThinkTrace"
}
```

### GET /api/search/stream
Server-Sent Events:
- `outline` - Initial struktur
- `content` - Inkrementell tekst
- `sources` - Kilder
- `done` - Ferdig

---

## Design System Referanse

### Farger
| Variabel | Neutral Light | Neutral Dark | Teal Light | Teal Dark |
|----------|--------------|--------------|------------|-----------|
| --bg-primary | #F4F6F8 | #141618 | #ffffff | #0f0f0f |
| --bg-secondary | #ffffff | #1e2022 | #f8fafa | #171717 |
| --text-primary | #1a1a1a | #e5e5e5 | #1a1a1a | #e5e5e5 |
| --accent | #6b7280 | #6b7280 | #20B2AA | #20B2AA |

### Spacing & Radius
- radius-sm: 8px
- radius-md: 12px
- radius-lg: 16px
- radius-xl: 24px

### Layout Breakpoints
- Mobile: < 640px (single column)
- Tablet: 640-1024px (stacked panels)
- Desktop: > 1024px (side-by-side panels)
