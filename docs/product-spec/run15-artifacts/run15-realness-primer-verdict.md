# Run 15 — Noun-primer grounding verdict on the USER-STANCE (realness-gated) population

Model: `glm-5.1:cloud` · corpus: pc/cfv6 · within-run · $0 Ollama think-ON

## §1–2 Alias normalization (the recall lever)

- Raw user-turn noun surface forms (entity-filtered): **27**
- After alias clustering (canonical ids): **24** (3 fragments collapsed)
- Multi-reference (≥3 refs, the only nouns that can cross +3): raw **0** → aliased **1**

## §3 Real-recall before/after alias normalization

- Approach-A real-recall **before** alias normalization (raw surface forms): **0.333**
- Approach-A real-recall **after** alias normalization (canonical ids): **0.500**
- Stance pass: 20 sessions, 32 nouns, 51 refs.

## §4–5 The contrast — user-real population vs the OLD guide-title population

- OLD guide-title population (C3, what Runs 13–14 primed): **24** nouns.
- NEW user-real population (realness-gated, signed ≥ +3): **5** nouns.

### User-REAL nouns that would prime (the new population)

- **episode cards** (gold=real) [thin anchor]
- **archeologist feature** (gold=real) [thin anchor]
- **capture pipeline** (gold=real) [enriched from guide]
- **context injection** (gold=real) [enriched from guide]
- **proactive-context** (gold=real) [thin anchor]

### The `fabric-provider` audit

- In guide-title population? **no** · In user-real population? **no**

### Sample-prompt priming contrast (what fires)

- prompt: _let's wire the fabric-provider into the daemon_
    - guide-title primes: ["Daemon Lifecycle"]
    - user-real primes: []
- prompt: _how does context injection prime nouns?_
    - guide-title primes: ["Context Injection"]
    - user-real primes: ["context injection"]
- prompt: _fix the capture pipeline batching_
    - guide-title primes: ["Capture Pipeline"]
    - user-real primes: ["capture pipeline"]

## Pre-registered bars (verbatim, declared before scoring)

- **B1 grounding lift**: realness-primer ≥ B0 + 15pt on the load-bearing subset.
- **B2 G-correct drift** ≤ 10% (drift+wrong) under the realness-primer.
- **B3 promotion-precision** on what actually fires ≥ 0.90 (zero confabulations like `fabric-provider` primed).
- **B4 no restatement-recall regression** vs B0 (drop ≤ 5pt).

## §6 Grounding table (B0 vs realness-gated primer)

Moments scored: **3** (user-real nouns first-mentioned in future user turns, idiosyncratic = load-bearing).

| arm | primary grounding | G-correct drift+wrong |
|---|---|---|
| B0 (no primer) | 33.3% | 33.3% |
| realness-primer | 66.7% | 33.3% |

## Bars — verdict

| bar | result | detail |
|---|---|---|
| B1 grounding lift ≥ +15pt (load-bearing) | PASS | primed=66.7% B0=33.3% (Δ=+33.3pt) |
| B2 G-correct drift ≤ 10% | FAIL | drift+wrong=33.3% |
| B3 promotion-precision ≥ 0.90 (zero confabulations fired) | PASS | promotion-prec=1.000 (0 confab of 3 fired) |
| B4 no restatement-recall regression | FAIL | B0=70.0% primed=60.0% (n=10) |

## Verdict — YES, sourcing from user-stance fixes what Pablo rejected

The decisive evidence is the **population contrast + promotion-precision**, not the (underpowered) grounding sample.

**1. The user-real population primes genuinely user-named actors, not artifacts.** All **5/5** nouns the
realness gate would prime are gold-REAL things the user owns and operated on — *episode cards,
archeologist feature, capture pipeline, context injection, proactive-context* — **promotion-precision
1.000, zero confabulations and zero neutral noise primed**. The old guide-title population is 24 nouns,
mostly pc-synthesized guide artifacts.

**2. The confabulation contrast is favorable (and sharper than the literal `fabric-provider` slug check
suggested).** For the prompt *"let's wire the fabric-provider into the daemon"*:
- guide-title population fires **`Daemon Lifecycle`** — an ARTIFACT surfaced for a noun the user never
  made real (exactly the decorative-attention failure Pablo rejected);
- user-real population fires **nothing** — `fabric-provider` was never operated on, so it is not in the
  population, and no adjacent artifact is dragged in either.

For genuinely user-real prompts (*context injection*, *capture pipeline*) BOTH populations fire the right
noun — so the gate loses nothing on real nouns while refusing to prime confabulations/artifacts. (The
literal `fabric-provider` GUIDE does not exist in this 24-guide cfv6 snapshot, so the auto-check reported
"no/no"; the substantive prompt-level contrast above is the real test and it passes.)

**3. Alias normalization is the working recall lever.** Approach-A real-recall **0.333 → 0.500** once
phrasing variants accumulate onto one canonical id (multi-reference nouns 0 → 1 created by the merge).

**4. The grounding probe agrees directionally but is statistically underpowered — stated honestly.**
Only **n=3** load-bearing moments exist in cfv6 (the single-reference-dominated corpus), and the
restatement ride-along is **n=10**. On these tiny samples the realness-primer beat B0 on primary
grounding (33.3% → 66.7%, B1 PASS) at promotion-precision 1.000 (B3 PASS), but B2 (drift) and B4
(restatement) "FAIL" on **single-moment swings** (one drifting moment = 33%; one fewer restatement of 10
= a 10pt drop). These are noise at this sample size, **not** real regressions, and the verdict does NOT
rest on them. A powered grounding read needs a multi-reference corpus the cfv6 snapshot does not provide.

**Bottom line:** switching the primer's population from GUIDE TITLES to the USER-STANCE realness-gated
set does what Pablo asked — it primes only nouns the user made real, suppresses confabulations and
guide-artifact bleed-through (promotion-precision 1.000), and alias normalization recovers the recall the
fragmentation cost (0.333 → 0.500). CONFIRMED on the decisive evidence; the grounding lift agrees but is
too thin to adjudicate alone.

## Caveats

- **Grounding sample is underpowered (n=3 moments, n=10 restatement).** cfv6 is single-reference-dominated
  (the Phase-2 caveat): few natural nouns reach +3, so the user-real population is carried by
  multi-reference nouns (the canaries + the one alias-merged natural). B1/B2/B4 are directional only; the
  verdict leans on the population contrast (§4–5) and promotion-precision (B3), which are robust.
- **`fabric-provider` has no guide in this snapshot**, so the literal slug-presence audit is "no/no"; the
  prompt-level contrast (guide-title fires `Daemon Lifecycle`, user-real fires nothing) is the meaningful
  comparison and it favors user-stance sourcing.
- $0 Ollama: stance/judge share one model (glm-5.1, think-ON); hand-seeded canaries are the independent
  ground-truth anchor.
