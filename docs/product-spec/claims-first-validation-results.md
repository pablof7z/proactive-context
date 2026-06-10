# Claims-First Validation Results

**Two corpora tested.** Run 3 = `nostr-multi-platform` (agent-orchestration corpus).
Run 4 = `proactive-context` (this repo's own dev history — richer, design-heavy, and the
corpus with known direction reversals for Probe 2). Runs 1–2 were null due to harness bugs.
**Judge / capture / compile model:** `ollama:glm-5.1:cloud`. **Embedder:** local fastembed (384-dim).
**Total OpenRouter spend across all four runs: $0.00** (everything ran on Ollama Cloud + local embed).

## Cross-corpus verdict summary

| | Run 3 — nostr (orchestration) | Run 4 — proactive-context (design) |
|---|---|---|
| HISTORY / FUTURE | 25 / 200 | 30 / 39 |
| P1 labels | 37 | 42 |
| P1 recall ALL (A / B) | 62.2% / 64.9% | 69.0% / 69.0% |
| P1 user-direction (A / B) | 62.5% / **81.2%** (n=16) | 66.7% / 33.3% (n=3) |
| P2 reversals scored | N/A (none in corpus) | **8** |
| P2 trajectory recoverable (A / B) | — | **7/8 / 3/8** |
| P3 latency reduction | −44% | −19% |
| §5 verdict | **PROMISING** | **FAILS (kill criterion) — but see caveats** |

**The two corpora disagree, and that disagreement is the finding.** Store B (claims-first) wins
on an *orchestration* corpus where the same user directions recur verbatim, but loses on a
*design* corpus where the value is in the supersession narrative — exactly what Probe 2 exposes.

---

# Run 4 — proactive-context (Probe 2 priority)

**Corpus:** `~/.claude/projects/-Users-pablofernandez-src-proactive-context`, 69 dated sessions,
chronological 80/20 split capped at 30 → HISTORY = oldest 30 (2026-05-28→05-29, the project's
dense design period), FUTURE = 39 (through 2026-06-10). Stores built fresh under an isolated
`PC_HOME`; the live `~/.proactive-context/projects/<key>` state and the repo were never touched.

**Self-referential guard (how the circularity was handled).** These transcripts can contain pc's
own injected `<system-reminder>Relevant project context …</system-reminder>` briefings and pasted
wiki-index dumps. A label or reversal mined from pc's *own* injection would be circular. The
transcript prep now (a) strips `<system-reminder>…</system-reminder>` spans (raw and HTML-escaped)
from every user turn before the judge sees it, and (b) drops any turn dominated by pc's derived
artifacts (the "Relevant project context (" briefing header, the "Derived cache — do not hand-edit"
/ "Rebuilt by proactive-context" wiki-index banner, or a `# Wiki Index` table). In practice this
corpus had almost no *live* injected briefings persisted into user turns (1 turn, and it was an
HTML-escaped paste inside a human message), so the guard mostly mattered as insurance — but it
ensures no label is sourced from machine output.

### Store contents (from 30 HISTORY sessions)
- Store B: 303 claims, 22 wiki guides. Store A: independently-built wiki guides.
- Reversal capture confirmed in-store: 28 compile/synthesize claims, 13 supersession claims,
  3 ratified claims — all three spec-named reversals are represented.

## Probe 1 — Restatement recall (42 verified labels; 39 implicit / 3 explicit)

| Cohort | Store A (wiki) | Store B (claims) |
|--------|---------------|------------------|
| **All (n=42)** | c14 / p15 / a13 → **69.0%** | c12 / p17 / a13 → **69.0%** |
| **Explicit / user-direction (n=3)** | c2 / p0 / a1 → 66.7% | c1 / p0 / a2 → 33.3% |
| **Implicit (n=39)** | c12 / p15 / a12 → 69.2% | c11 / p17 / a11 → **71.8%** |

On overall recall the stores are **tied (69% each)**; B slightly leads on implicit facts. The
"user-direction" cohort is only **n=3** here (this corpus's human turns are mostly design
discussion, not standing directives), so the 66.7%→33.3% gap is a one-label swing and is NOT a
reliable signal — unlike Run 3 where n=16 made the user-direction win meaningful.

## Probe 2 — Direction-change fidelity (THE PRIORITY; 8 reversals, all verified)

The reversal miner found 8 real reversals from the store (all with both X and Y verifiable),
including the capture-pipeline redesign (3-step distill→plan→apply ⇒ single tool-agent loop) and
the capture-evidence format change (free-form quotes ⇒ transcript line-range Rust slicing) — the
EXTRACT and citation-anchoring evolutions the spec hinted at. Examples:
- *Embedding provider:* OpenAI 1536-dim via OpenRouter ⇒ local all-MiniLM-L6-v2 (384-dim).
- *Primary command:* `generate` (with Ask/Search roles) ⇒ removed; `inject` is now primary.
- *Inject hook language:* TypeScript ⇒ Rust.

| Metric | Store A (wiki) | Store B (claims) |
|--------|---------------|------------------|
| asserts current Y | **8/8** | 6/8 |
| leaks stale X as current (the sin) | 1/8 | 2/8 |
| **trajectory X→Y recoverable** | **7/8** | **3/8** |

**Store A (wiki) wins Probe 2 decisively.** Concrete example (embedding provider): Store A's
briefing says *"This configuration replaced OpenAI's 1536-dim model via OpenRouter, which was
previously supported"* — the prose guide retains the supersession narrative. Store B correctly
asserts the current local-embedding truth but **drops the "replaced OpenAI" history**, so the
X→Y trajectory is unrecoverable. This is structural: reconciliation into prose naturally writes
"current Y (was X)", whereas retrieving top atomic claims surfaces the latest claim and leaves
the prior state behind. B also leaks a stale direction as current twice (vs A's once).

## Probe 3 — Operational metrics (42 inject runs/store)

| Metric | Store A (wiki) | Store B (claims) | Δ |
|--------|---------------|------------------|---|
| p50 latency | 3839 ms | 3103 ms | −19% |
| p95 latency | 13757 ms | 6729 ms | −51% |
| total tokens in | 194,087 | 64,749 | **−67%** |
| total tokens out | 13,312 | 8,175 | −39% |
| incoherent / fact-confetti | — | 0/42 | — |

B is still much cheaper on tokens (−67% in) and tail latency (−51% p95), but only −19% at p50 —
below the 30% bar on this corpus (Store A's guides here are smaller/faster to compile than the
nostr corpus's, narrowing the median gap).

## §5 Pre-registered read (applied verbatim) — Run 4

- **P1 user-direction recall ≥ Store A** — **FAIL** (33.3% vs 66.7%), but **n=3** — a single-label
  swing, not a reliable signal on this corpus.
- **P2 strictly fewer stale leaks than Store A** — **FAIL** (B leaks 2/8 vs A 1/8; and B recovers
  trajectory on only 3/8 vs A's 7/8). This is the robust, meaningful result.
- **P3 ≥30% latency reduction** — **FAIL** (−19% at p50).
- **Coherence <20% incoherent** — **PASS** (0/42).

**Overall verdict (Run 4): FAILS — Store B does not clear the bar on this corpus.** The decisive,
statistically-real reason is Probe 2: claims-first loses the supersession trajectory that prose
guides preserve. The P1 user-direction "fail" is real per the rule but rests on n=3 and should not
be over-read; overall P1 recall is tied at 69%.

## What Run 3 + Run 4 together say

Claims-first is **not** a universal win. It shines when the payload is recurring atomic user
direction (Run 3: +19pts user-direction recall, −44% latency) but regresses when the payload is an
evolving design with reversals (Run 4: −4/8 on trajectory recovery). A claims store that wants to
match prose on Probe 2 needs to render supersession explicitly ("current Y (was X)") at compile
time, not just retrieve the latest claim in a cluster — which is exactly the supersession-rendering
the original claims-first proposal called for but this Phase-0 compile path does not yet do.

---

# Run 3 + earlier — nostr-multi-platform (and the Run 1-2 nulls)


**Corpus:** `nostr-multi-platform` (`/Users/pablofernandez/Work/nostr-multi-platform`), 225 sessions
**Judge / capture / compile model:** `ollama:glm-5.1:cloud` (Ollama Cloud)
**Embedder:** local fastembed (384-dim)
**Date:** 2026-06-10
**Headline verdict (Run 3, §5 applied verbatim):** **PROMISING — all three evaluable criteria pass.**

This experiment took three runs. Runs 1–2 were null due to two harness bugs in the
label miner; Run 3 (after the fixes) produced a real, scoreable signal. All three runs
are recorded below for honest history.

---

## Spend

**Total OpenRouter spend across all runs: $0.00.** Every LLM call (triage, EXTRACT,
authority-tagging, route/reconcile, label-mining judge, inject-compile, recall judge)
ran through the user's configured `ollama:glm-5.1:cloud` endpoint, not OpenRouter.
Embeddings ran on the local fastembed model. The only cost was wall-clock + Ollama Cloud
usage. Store builds (Run 3): Store B 2454s, Store A 2452s. Scoring 37 labels: ~12 min.

---

## Run history

| Run | HISTORY | Judge HISTORY context | Labels mined | Outcome |
|-----|---------|----------------------|--------------|---------|
| 1 | 10 sessions | ~4 KB raw transcript | 0 | INCONCLUSIVE (bug) |
| 2 | 10 sessions | 29 KB store-derived | 0 | INCONCLUSIVE (bug) |
| 3 | 25 sessions | 115 KB store-derived | **37** | **PROMISING** |

### What was wrong in Runs 1–2 (and fixed for Run 3)

1. **History context was unintelligible (fixed Run 2).** `build_history_summary` fed the
   judge raw transcript text — for this corpus that is mostly terse one-line commands and
   tool-notification XML. Fix: build the judge's HISTORY context from the captured stores
   (Store A wiki guide bodies + Store B claim assertions). Run 2 raised the context from
   ~4 KB to 29 KB but still mined 0 — because of bug #2.

2. **The future transcript was never parsed (the real root cause; fixed Run 3).**
   `mine_labels` called `parse_transcript(&raw)` passing *file content*, but
   `parse_transcript` takes a *file path* (it reads + parses the JSONL itself). Every
   future session therefore errored at the parse step and was silently `continue`d — the
   judge never saw a single FUTURE transcript in Runs 1–2. The "20/20 sessions" log line
   was the loop iterating; each iteration bailed before the judge call. Fix: pass the path.

3. **Two supporting fixes in Run 3:** (a) the mining prompt was broadened to count oblique
   references and questions ("didn't we decide to use outbox?", "use rust-nostr's nip44,
   don't reimplement") as restatements, not just verbatim re-explanations; (b) a
   `extract_human_turns` helper now feeds the judge the actual human conversational turns
   (dropping the giant bootstrap directive and tool-notification blobs) instead of a
   raw-transcript-head truncation that cut off the real back-and-forth.

4. **Verdict-mapping bug (fixed in code).** The null case (no verified labels) mapped to
   "FAILS"; it now maps to "INCONCLUSIVE". A loss on P1 still maps to FAILS; genuine
   pass/mixed cases are unchanged.

5. **Frozen-label reuse (added).** `--score-only` now loads an existing `labels.jsonl`
   instead of re-mining, so the label set is frozen before scoring (per spec §4) and
   scoring is cheaply re-runnable.

---

## Run 3 — the scoreable run

### Corpus split

| | |
|---|---|
| Total sessions | 225 |
| HISTORY (cap 25, spec allows ~30) | 25 |
| FUTURE | 200 |
| FUTURE scanned for labels | 20 (cap) |
| Verified labels | 37 |
| Label authority split | 16 explicit / 21 implicit |

Store B: 342 claims across 192 clusters, 27 wiki guides. Store A: 23 wiki guides
(built independently, no claim tap).

### Frozen label set — examples

1. `[explicit]` *Subscriptions should be aggregated/batched (like NDK does, ~100ms intervals) to prevent many small subscriptions.*
   future prompt: "yes, this should follow the same subscription aggregation logic that we build to prevent sending tons of small…"
   history evidence: "outbox model"
2. `[explicit]` *No specific relay (including the damus relay) should be hardcoded; routing must use the outbox model.*
   future prompt: "we are supposed to be using outbox, so we are supposed to be connecting to the relays the user actually uses…"
3. `[implicit]` *DiagnosticsView is the home for all diagnostic UI in Chirp.*
   future prompt: "on chirp, in diagnostics, I want to be able to tap a subscription and see what's inside the subscription…"

(Full set: `labels.jsonl`, 37 rows.)

### Probe 1 — Restatement recall (recall = contained + partial)

| Cohort | Store A (wiki) | Store B (claims) |
|--------|---------------|------------------|
| **All (n=37)** | contained 8 / partial 15 / absent 14 → **62.2%** | contained 7 / partial 17 / absent 13 → **64.9%** |
| **Explicit / user-direction (n=16)** — the sin meter | contained 3 / partial 7 / absent 6 → **62.5%** | contained 3 / partial 10 / absent 3 → **81.2%** |
| **Implicit (n=21)** | contained 5 / partial 8 / absent 8 → **61.9%** | contained 4 / partial 7 / absent 10 → **52.4%** |

**The kill criterion is user-direction recall: Store B 81.2% ≥ Store A 62.5% — B wins by ~19 points.**
On the absolute-sin cohort B leaves only 3/16 facts absent vs A's 6/16. B trades some
implicit-fact recall (52.4% vs 61.9%) for a large gain on explicit user direction — the
exact priority the project's failure hierarchy demands.

### Probe 2 — Direction-change fidelity (SHOULD HAVE)

**N/A on this corpus / window.** Store B's 192 clusters are overwhelmingly co-occurring
topical facts from the same session/date; the 25-session HISTORY window contains no clean
temporal reversal (user established X on date 1, overrode with Y on date 2) with differing
timestamps to score X→Y trajectory + stale-leak. The one observed supersession (file-length
limit 300→500 LOC) appeared as a single in-place wiki revise, not a multi-version claim
cluster. Probe 2 is left unscored rather than fabricated; it needs a corpus with explicit
documented reversals (or a longer window) to exercise.

### Probe 3 — Operational metrics (n=37 inject runs per store)

| Metric | Store A (wiki) | Store B (claims) | Δ |
|--------|---------------|------------------|---|
| p50 latency | 7140 ms | 3968 ms | **−44%** |
| p95 latency | 13195 ms | 7450 ms | −44% |
| total tokens in | 143,250 | 55,770 | **−61%** |
| total tokens out | 8,057 | 4,940 | −39% |
| incoherent / fact-confetti briefings | — | 0 / 37 | — |

Store B is materially cheaper and faster: roughly half the latency and ~40% of the input
tokens, because claim retrieval feeds the compile model pre-ranked atomic facts instead of
whole prose guides.

---

## §5 Pre-registered read (applied verbatim)

Store B is *promising* if, on the frozen label set:
- **Probe 1 recall on user-direction labels ≥ Store A** — **PASS** (81.2% vs 62.5%; a loss here would kill the proposal).
- **Probe 2: strictly fewer stale-assertion leaks than Store A** — **N/A** (no reversals in this corpus/window; not scored).
- **Probe 3: ≥30% latency reduction** — **PASS** (44% faster at p50).
- **Briefing coherence: judge flags incoherent/fact-confetti on <20% of B briefings** — **PASS** (0%).

**Overall verdict: PROMISING — all evaluable criteria pass; the kill criterion (user-direction
recall) is cleared with margin.** The one unmet item (Probe 2) is unmet because the corpus
lacks the phenomenon, not because Store B failed it.

---

## Honest caveats

- **Single corpus, single judge, n=37.** This is a directional signal, not proof. The judge
  is one strong model with the verdict prompt logged; no multi-judge panel (deferred to v2).
- **Two EXTRACT passes, not one.** The spec intended the claim tap to ride the same pass that
  builds the wiki (one EXTRACT spend, two stores). The implementation builds Store B then
  Store A as separate passes — doubling build cost and letting routing non-determinism
  slightly diverge the two stores' guide sets. The stores are still built from the *same 25
  HISTORY sessions*, so the comparison is fair, but a true single-pass tap would tighten it.
- **Label miner is generous by design.** It counts oblique references/questions as
  restatements; verification is store-representation token-overlap (≥60%) or a 6-word phrase
  match, which can admit loosely-related facts. Labels are frozen in `labels.jsonl` for audit.
- **Implicit recall regressed.** B underperforms A on implicit facts (52.4% vs 61.9%). Worth
  understanding before any rollout — claims retrieval may under-surface inferred-but-unstated
  observations that prose guides capture in passing.

## Raw artifacts

- Results (machine copy): `~/.proactive-context/experiments/cfv3-20260610-175752/claims-first-validation-results.md`
- Frozen labels (37): `…/cfv3-20260610-175752/labels.jsonl`
- Probe results (per-label briefings + verdicts + timings): `…/cfv3-20260610-175752/probe_results.jsonl`
- Judge HISTORY context (115 KB): `…/cfv3-20260610-175752/history_context.txt`
- Store A wiki (23 guides): `…/cfv3-20260610-175752/store-a/`
- Store B claims (342) + wiki (27 guides): `…/cfv3-20260610-175752/store-b/`
- Split manifest: `…/cfv3-20260610-175752/split_manifest.json`
- Run 1 store: `~/.proactive-context/experiments/cfv2-20260610-173322/` (10-session, 0 labels)
- Eval logs: `/tmp/eval-run3.log` (build), `/tmp/eval-score3.log` (frozen-label scoring)
