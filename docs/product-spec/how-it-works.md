# How proactive-context Works (High-Level)

**As of 2026-06-12.** This describes the *landed, validated* architecture — every default below was flipped on the basis of a measured result (evidence trail: `claims-first-validation-results.md` Runs 1–9, `claims-first-learnings.md` F1–F16).

## The problem it solves

A human spends months giving direction to AI coding agents — decisions, corrections, changes of mind — in moment-bound conversation that evaporates when each session ends. `pc` sits between the human and the agent and performs two conversions:

- **Capture (decontextualization):** turn moment-bound utterances into moment-free knowledge — facts that stay true and intelligible outside the conversation that produced them.
- **Inject (attention allocation):** at the moment the agent is about to act, push exactly the accumulated direction that bears on that action.

Governing principles: **proactivity** (delivery is system-driven push at critical points — never agent-discretionary pull, never passive context files; underinjection is the cardinal failure), **integrity by construction** (models point at transcript line numbers; Rust slices the verbatim text — fabricated evidence is unrepresentable), and **keep everything** (supersession links and demotion, never deletion).

## The knowledge store: four artifact types, one wiki directory

All under `<project>/docs/wiki/`, each earning its place for a different knowledge shape:

| Artifact | Shape | Mutability | Job |
|---|---|---|---|
| **Guides** (`<slug>.md`) | Topic prose, cited | Reconciled in place | Current truth per topic |
| **Episode cards** (`episodes/`) | Prior State → Trigger → Decision → Consequences arc | Immutable | Direction changes, reversals, root causes — *why* things are the way they are |
| **Research records** (`research/`) | Verbatim-sliced investigation reports | Immutable | Experiments/evals with method + verdict |
| **Claim log** (`~/.proactive-context/.../claims.jsonl`) | Atomic cited claims, authority-tagged | Append-only | Lossless substrate under everything; future re-projection and eval |

Why this mix (the empirical short version): prose reconciliation is the only mechanism that reliably maintains current truth (write-time contradiction detection survived three replication attempts); episode cards are the best direction-change source ever measured here (6/8 trajectory, 0/8 stale leaks — narrative arcs encode supersession structurally); the claim log proved lossless in every run and costs no LLM; research records preserve investigation structure that atomic extraction shreds.

## Capture (session end / Stop hook, off the hot path)

```
transcript ──> TRIAGE (cheap model: anything worth capturing?)
          ──> EXTRACT (atomic cited claims; subagent task-results visible)
          ──> authority tagging (mechanical: user line = explicit, agent line = implicit)
          ──> claim-log tap (append + embed; zero LLM)
          ──> ROUTE (embedding recall + LLM rerank → home guide per claim)
          ──> RECONCILE (per guide: add/revise with supersession breadcrumbs)
          ──> research recognition (immutable records, precision-gated)
          ──> episode recognition (immutable cards, salience-gated: product movement only)
          ──> structural maintenance (index, links, re-embed)
```

Everything is cited to transcript line ranges and verified in Rust before any write.

## Inject (every prompt / UserPromptSubmit hook, hot path)

```
prompt ──> trivial-prompt gate
       ──> vector retrieval (local embeddings, sqlite-vec)
       ──> catalog: guides + episode cards (typed `episode:` rows) + committed docs
       ──> SELECT (fast model): picks sources; routes why/history questions to cards,
           present-tense questions to guides; resolves the prompt into a standalone query
       ──> COMPILE (strong model): dense cited briefing; cards labeled as historical
           provenance unless corroborated; per-session ledger = only NEW facts each turn
       ──> <system-reminder> pushed into the agent's context
```

Composition happens at SELECT (the model picks per-prompt among typed sources) — never by blending retrieval budgets, which measurably dilutes.

## Cold start

`pc archeologist` replays existing transcript history (Claude Code, Codex, opencode sources) chronologically through the identical capture pipeline, stamping artifacts with their historical session dates. Months of past direction become the wiki retroactively.

## What is deliberately NOT in the architecture

- **Claims-as-store / projection-from-log** as the primary substrate — rejected by Runs 4–7 (trajectory loss; cluster fragmentation).
- **Delta-EXTRACT** (typed change-ops at extraction) — mechanism *proven* (6/8 reversal diagnostic), implementation flagged off pending cost/precision fixes. The named next swing.
- **Pull tools, context-file dumps, uncertainty-gated injection** — rejected on the proactivity principle.
- **Whole-store cache-resident injection** — rejected: context window is attention; telling a model to attend to everything means it attends to nothing.

## How quality is judged

The standing instruments, all built on temporal holdout over real history with pre-registered criteria: restatement recall (did the user have to repeat themselves?), direction-change fidelity (current truth asserted, no stale leak, trajectory recoverable), **predict-the-correction** (the North Star: can the store predict the user's correction before they make it?), and attention-efficiency (is injected content counterfactually load-bearing?). The benchmark pair (wallet + pc corpora, frozen labels) is the regression suite for any pipeline change.
