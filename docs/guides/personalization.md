# Personalization & Multi-Tenant Ranking

Prism is intentionally **user-agnostic and stateless in its ranking**. The same
query against the same collection returns the same candidate set and baseline
score for every user. This is a deliberate design property, not a limitation: it
makes Prism a clean, privacy-respecting relevance core that any product can layer
its own personalization on top of.

This guide explains how to build user-level personalization *on top of* Prism —
without modifying Prism itself — using the pattern used by privacy-first search
products where each user chooses their own personalization level (from
DuckDuckGo-style anonymity to full personalization).

> **See also:** [Ranking & Boosting](ranking.md) for Prism's built-in
> (collection-level) scoring, and [Search](search.md) for the request/response
> shapes referenced below.

---

## The core principle

```
┌──────────────────────────────────────────────────────────┐
│  User-facing app (personalization level slider: 0 → max) │
└────────────────────────┬─────────────────────────────────┘
                         │ query + personalization level
┌────────────────────────▼─────────────────────────────────┐
│  Product layer (your app / API gateway)                  │
│                                                          │
│  • Level 0 (anonymous mode): no rerank, pass Prism order │
│  • Level >0: post-fetch rerank mixing in user signals    │
│    (geo, click history, profile, session), weighted by   │
│    the chosen level                                      │
└────────────────────────┬─────────────────────────────────┘
                         │ POST /collections/:collection/search
                         │ (neutral, identical for all users)
┌────────────────────────▼─────────────────────────────────┐
│  Prism — user-agnostic, stateless ranking                │
│                                                          │
│  • Hybrid retrieval (text + vector) + RRF                │
│  • Recency decay, field boost, document boost            │
│  • Cross-encoder rerank, collapse, near-dup              │
│  • Returns ALL fields + score → for downstream rerank    │
└──────────────────────────────────────────────────────────┘
```

### Why this works

1. **Prism never changes based on who is asking.** Same query + same
   collection = same results. This is exactly what anonymous mode (level 0)
   requires — and it is Prism's default.
2. **Personalization is additive.** Your product requests an over-fetch from
   Prism (e.g. `limit: 60`), then reranks in memory and returns the top `N`.
   The level controls *how many* and *which* signals are mixed in.
3. **The privacy story is clean.** No personal data ever reaches Prism — all
   user-specific state stays in your product layer. This makes a stronger
   "truly anonymous" claim for level 0 and simplifies compliance.

---

## Prism features that support this pattern

You do not need anything new from Prism. These existing features are what make
the pattern practical:

- **`SearchRequest.fields` are returned in full.** Your product can read signal
  fields (`domain_authority`, `hop_depth`, `crawled_at`, custom fields) for
  local reranking without extra lookups.
- **`limit` over-fetching.** Prism can deliver a wide candidate set (e.g. 3× the
  displayed count); your product selects and shapes it.
- **`SearchRequest.score_function`.** A per-query arithmetic expression that can
  fold a single document-level signal into the score directly inside Prism
  (e.g. `"_score * (1 + 0.3 * geo_boost)"`).
- **`collapse` / `near_dup`.** Prism can deduplicate by field value or semantic
  similarity *before* results reach your product, keeping the candidate set clean
  regardless of personalization level.
- **Per-request `rerank` override.** Cross-encoder reranking can be enabled or
  disabled per request, so neutral mode can skip expensive models.

### What does *not* belong in Prism

Per-user profiles, click history, and personalization policy live in your
product layer. Prism's `RankingConfig` is collection-level (not per-query
per-user), and that is an *advantage* for this architecture: it keeps Prism
user-agnostic and the policy centralized in one place.

---

## Strategy overview

| Approach | How the level affects results | Best for |
|---|---|---|
| **A. Pure cutoff** | Level 0 = raw Prism score; level >0 = combine with signals | Minimal, little personal data |
| **B. Weighted blend** | The level sets the weight of personal vs. neutral signals | The natural "slider" model |
| **C. `score_function` in Prism** | Push signal weight *down* into Prism per-query | When the signal is a single document field |

Below, strategies A and B are shown in Rust and TypeScript (product-layer
rerank), and C as a raw Prism request.

---

## Step 1 — Over-fetch from Prism (level-independent)

Always request a wider candidate set than you intend to display. This is the
*only* call to Prism — the rest happens locally. Note that no user id is sent.

```bash
# Fetch 3× as many candidates as we will display. collapse removes dups before
# they reach us. score_function is NEUTRAL here — no user-specific weighting.
curl -X POST http://localhost:3080/collections/pages/search \
  -H 'Content-Type: application/json' \
  -d '{
    "query": "monero wallet",
    "limit": 60,
    "collapse": { "field": "url", "max_docs": 1 },
    "highlight": {
      "fields": ["content", "title"],
      "pre_tag": "<mark>",
      "post_tag": "</mark>"
    }
  }'
```

Prism returns up to 60 neutrally-ranked results, all fields included. Now the
personalization begins — *after* the response.

---

## Step 2A — Level-based rerank in the product layer (Rust, recommended)

This builds on Prism's neutral score, extending it with a weighted term for
personal signals. The level (`0.0`–`1.0`) controls how much the personal
signals contribute. At level 0, personal signals are absent and results keep
Prism's order.

```rust
/// A Prism search result with the fields we need for reranking.
struct PrismResult {
    id: String,
    score: f32,                       // Prism's neutral score
    fields: std::collections::HashMap<String, serde_json::Value>,
}

/// User-specific signals. All optional: absent for anonymous / level-0 users.
struct UserSignals {
    geo_country: Option<String>,
    click_affinity: Option<std::collections::HashMap<String, f32>>, // domain -> affinity
    session_terms: Option<Vec<String>>,
}

/// Weighted blend of Prism's neutral score and personal signals.
/// level: 0.0 (anonymous) .. 1.0 (max personalization).
fn rerank_by_level(
    prism: Vec<PrismResult>,
    level: f32,
    signals: &UserSignals,
    display_limit: usize,
) -> Vec<PrismResult> {
    let level = level.clamp(0.0, 1.0);
    let personal_weight = level * 0.5;          // at most 50% personal
    let neutral_weight = 1.0 - personal_weight;

    let mut ranked: Vec<_> = prism
        .into_iter()
        .map(|mut r| {
            let personal = personal_score(&r, signals);
            let blended = r.score * neutral_weight + personal * personal_weight;
            r.score = blended; // overwrite with the blended score
            r
        })
        .collect();
    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(display_limit);
    ranked
}

/// Combine available personal signals, each in [0, ∞).
/// Returns 0.0 when the user has no signal (anonymous / level 0).
fn personal_score(r: &PrismResult, s: &UserSignals) -> f32 {
    let mut score = 0.0_f32;

    // Geo match: boost documents from the user's region.
    if let (Some(want), Some(got)) = (&s.geo_country, r.fields.get("country").and_then(|v| v.as_str())) {
        if want == got { score += 0.5; }
    }

    // Click affinity: boost domains the user often clicks.
    if let (Some(affinity), Some(url)) = (&s.click_affinity, r.fields.get("url").and_then(|v| v.as_str())) {
        if let Some(domain) = domain_of(url) {
            score += affinity.get(&domain).copied().unwrap_or(0.0);
        }
    }

    // Session context: boost documents sharing terms with recent queries.
    if let Some(terms) = &s.session_terms {
        if shares_term(r, terms) { score += 0.2; }
    }

    score
}

fn domain_of(_url: &str) -> Option<String> { /* … */ None }
fn shares_term(_r: &PrismResult, _terms: &[String]) -> bool { /* … */ false }
```

**What each level gives concretely:**

- `level = 0.0` → `personal = 0` → order = Prism's (DuckDuckGo-equivalent).
- `level = 0.3` → mild geo/language boost, no click history.
- `level = 1.0` → full blend of click, session, and profile signals.

`UserSignals` is constructed *only* for logged-in users at level > 0. Anonymous
visitors automatically get the pure neutral rank because their signals are empty.

---

## Step 2B — Level-based rerank in TypeScript

The same logic, useful when the product layer is a Node/Bun service or a
client-side demo:

```typescript
interface PrismResult {
  id: string;
  score: number;
  fields: Record<string, unknown>;
}

interface UserSignals {
  geoCountry?: string;
  clickAffinity?: Map<string, number>; // domain -> affinity
  sessionTerms?: string[];
}

/**
 * Weighted blend of Prism's neutral score and personal signals.
 * level: 0 (anonymous) … 1 (max personalization).
 */
export function rerankByLevel(
  prism: PrismResult[],
  level: number,
  signals: UserSignals,
  displayLimit = 20,
): PrismResult[] {
  const clamped = Math.min(1, Math.max(0, level));
  const personalWeight = clamped * 0.5;
  const neutralWeight = 1 - personalWeight;

  return prism
    .map((r) => {
      const personal = personalScore(r, signals);
      return { ...r, score: r.score * neutralWeight + personal * personalWeight };
    })
    .sort((a, b) => b.score - a.score)
    .slice(0, displayLimit);
}

function personalScore(r: PrismResult, s: UserSignals): number {
  let score = 0;
  // Geo match: boost documents from the user's region.
  if (s.geoCountry && (r.fields.country as string) === s.geoCountry) score += 0.5;
  // Click affinity: boost domains the user often clicks.
  const domain = domainOf(r.fields.url as string);
  if (s.clickAffinity && domain) score += s.clickAffinity.get(domain) ?? 0;
  // Session context: boost documents sharing terms with recent searches.
  if (s.sessionTerms && sharesTerm(r, s.sessionTerms)) score += 0.2;
  return score;
}
```

---

## Step 2C — Alternative: push signal weight into Prism via `score_function`

When the personal signal can be expressed as a single document field, the
weighting can be moved *into* Prism per-query via `score_function`. This is more
limited (one expression only, no combination of several signals) but avoids the
product layer having to load all fields to rerank.

```bash
# "geo_boost" is a document field (0.0–1.0). The user's level (0.3) scales how
# much it counts. Level 0 = no effect since the term becomes 0.
curl -X POST http://localhost:3080/collections/pages/search \
  -H 'Content-Type: application/json' \
  -d '{
    "query": "monero wallet",
    "limit": 20,
    "score_function": "_score * (1 + 0.3 * geo_boost)"
  }'
```

Here `0.3` is the user's chosen level. Pros: no post-fetch rerank needed.
Cons: `score_function` cannot see user-specific signals (such as click history)
— only document fields. Use C for document-near signals (geo, language) and A/B
for genuinely personal signals (clicks, session, profile).

---

## Recommended starting point

1. **Start with strategy A** (pure cutoff): level 0 = raw Prism score, and a
   single personal signal (geo/language) turns on from a low level. Minimal
   personal data, fast to build.
2. **Extend to strategy B** when you want the slider to mix several signals —
   click and session come in naturally here.
3. **Use strategy C** (`score_function`) selectively for document-near signals
   like geo/language, to offload the product layer.

Whichever strategy you choose, **Prism stays unchanged** — and that is the
point.
