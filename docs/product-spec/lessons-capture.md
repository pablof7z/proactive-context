# proactive-context — Lessons Capture & Injection

**Status:** Proposed (v0.2)

**One-line description:**
A post-conversation capture loop that distills durable *lessons* from agent transcripts into structured markdown notes, which are then indexed and proactively re-injected into future conversations — turning proactive-context from a static-notes RAG into a self-improving memory.

---

## Problem

The v0.1 system indexes markdown the user *already wrote*. But the highest-signal knowledge is generated continuously inside AI coding conversations and then thrown away:

- Corrections the user gives ("no, we use bun not npm here") are forgotten the moment the session ends.
- Hard-won fixes (an error → root cause → fix) have to be rediscovered next time.
- Decisions and their rationale evaporate; the next session starts cold.

The result: the user re-teaches the assistant the same things repeatedly, and the assistant re-solves the same problems. The knowledge exists — it's just never captured or resurfaced.

**The golden rule this system is built to enforce:**

> Every time the user has to correct the system is a moment where the system must learn, so the user never has to say the same thing again. LLM tokens are infinitely cheaper than human time.

A correction is not a nuisance — it is the highest-signal event the system can receive. Every "no, not like that", every preference stated, every rule violated and caught, every implementation pattern rejected — all of it should be captured and made available to every future session. The cost of capturing and re-injecting that knowledge is negligible. The cost of not capturing it is compounding: the user pays the correction tax again and again.

---

## Solution

A two-part loop layered on top of the existing v0.1 engine:

1. **Capture (new, async, off the hot path):** a `SessionEnd` hook runs *once*, after a conversation completes. It scans the transcript, extracts a small set of durable **lessons**, and writes them as structured markdown notes.
2. **Inject (existing, fast, hot path):** the existing `UserPromptSubmit` hook semantic-searches those lesson notes (alongside ordinary notes) and injects the relevant ones before the model sees the next prompt.

### Capture trigger: `SessionEnd`, not `Stop`

This is a load-bearing choice. Claude Code exposes both `Stop` (fires every time the assistant finishes a response — many times per session) and `SessionEnd` (fires once, when the session ends). Capture **must** run on `SessionEnd`:

- Running on `Stop` would re-distill the entire transcript on *every* turn → N LLM calls per session, escalating cost, and duplicate/overlapping lessons.
- `SessionEnd` gives exactly one distillation pass over the complete transcript — one call, no duplication, full context.

A future "fresher capture" mode could use `Stop` with a debounce + a per-session dedup guard (only distill turns added since the last capture), but that is explicitly out of v0.2 scope.

The governing principle is **distill, don't dump.** We never raw-index conversation logs — that produces contradiction, staleness, and noise (abandoned tangents and reversed decisions retrieved as if live, plus a self-reinforcing loop if the assistant's own output is fed back). Instead, capture refines the messy transcript stream into a few high-signal lessons *before* anything becomes retrievable.

Capture and injection are deliberately decoupled: capture is slow and quality-controlled and runs when no one is waiting; injection is fast and runs synchronously on every prompt.

---

## Product Memory — The Accumulating Project Model

Individual lessons are the *atoms* of capture. But the larger goal is something richer: over time, as lessons accumulate, the system builds up a **product model** per project — a synthesized, living document of everything the system has learned about how this user wants this product to work.

The product model is the answer to: *"If a new developer joined this project, what would they need to know that isn't in the code — and specifically, how does this user want things implemented, designed, and decided?"*

### What the product model captures

- **Implementation patterns** — how the user wants specific things built: component structure, naming conventions, state management approach, preferred libraries and why.
- **Rejected approaches** — what has been tried and overridden, with rationale. Critical: if the assistant suggests the same bad approach twice, it has failed.
- **Product rules** — how things should look, behave, and be structured: UI patterns, error handling conventions, API design choices, testing expectations.
- **Preferences and working style** — how the user wants to collaborate, what level of autonomy is expected, what must always be confirmed.
- **In-progress decisions** — architectural choices being made, open questions, things explicitly left undecided.

### Product model vs individual lessons

These are complementary layers, not the same thing:

| | Individual lesson | Product model |
|--|--|--|
| **Granularity** | One event, one rule | Synthesized state of all knowledge |
| **Updated** | Once (when captured) | Re-synthesized after each capture pass |
| **Used for** | Semantic retrieval (relevant when query matches) | Injected at session start (always-on context for the project) |
| **Format** | Structured lesson block | Coherent narrative document |

### Synthesis pass

After each capture pass writes individual lessons, a second step runs: read the current product model + the new lessons → produce an updated product model. This is a synthesis call, not just an append. The new model may:

- Promote a recurring lesson into a permanent principle
- Strengthen a weakly-held rule that has now been confirmed multiple times
- Flag a contradiction between an old principle and a new correction (see below)
- Update an in-progress decision if this session resolved it

The synthesized product model is stored at:
```
~/.proactive-context/projects/<normalized-path>/PRODUCT_MODEL.md
```

It is injected at **session start** for that project (always-on, not semantic-search-gated) because it is compact, authoritative context — the assistant should never begin a session in that project without knowing it.

---

## Contradiction Handling — Preserve, Surface, Reconcile

Contradictions are **not resolved automatically.** When a new lesson conflicts with an existing one, the system's job is to preserve both and surface the conflict — not to silently overwrite history.

### Why preserve contradictions

A contradiction between an old preference and a new one is not an error to be hidden. It is information:
- The user may have genuinely changed their mind.
- The rule may be context-dependent (applies in case A, not case B).
- The newer lesson may be a mistake, and the user would want to know before it takes effect.

Silently replacing old lessons with new ones discards this signal. Surfacing the contradiction gives the user and the assistant a chance to make it explicit.

### How contradictions are handled

1. **Detection at capture time.** During the synthesis pass, if a new lesson's Rule field contradicts an existing lesson's Rule, both are flagged with `status: contradiction` and linked to each other via `contradicts: <slug>`.

2. **Both are preserved in the lesson store.** Neither is deleted or demoted to inactive. The raw-immutable principle applies: the truth of what was said at each point in time is preserved.

3. **Surfaced at injection time.** When both a lesson and its contradiction are retrieved for the same query, they are injected together with an explicit `[CONTRADICTION]` marker:

```
[CONTRADICTION] Two conflicting lessons on this topic:
  • (2025-11-01, warm) Rule: Always use server components for data fetching.
  • (2026-03-14, warm) Rule: Use client components for dashboard widgets — server components caused hydration issues here.
  The newer lesson (2026-03-14) may supersede the older, or these may be context-dependent.
```

4. **The assistant navigates or asks.** With the contradiction explicitly in context, the assistant can either:
   - Resolve it using available context (newer date + same domain → newer likely supersedes)
   - Ask the user directly: *"I have conflicting guidance on X — [old rule] vs [new rule]. Which applies here, or should I treat these as context-dependent?"* — and capture the user's answer as a new lesson that reconciles the two.

5. **Product model flags contradictions explicitly.** The product model synthesis pass does not paper over contradictions — it surfaces them in a dedicated section of `PRODUCT_MODEL.md` so they are visible and inviting resolution.

### The contradiction lifecycle

```
new lesson written → synthesis detects conflict → both flagged contradiction
        ↓
injected together at next relevant session
        ↓
user/assistant reconciles (or user explicitly confirms one wins)
        ↓
winning lesson gets status: active, losing gets status: superseded
contradiction flag cleared, product model updated
```

This is the opposite of a naive "last-write-wins" system. The user's changing views over time are a signal — the system's job is to make them visible, not to decide unilaterally which one is right.

---

## The Lesson Format

Each lesson is a structured markdown unit. The format enforces the distillation discipline — the **Rule** field is the generalizable principle, distinct from the specific **Fix**.

```markdown
---
type: lesson
category: correction        # error-fix | correction | discovery | config | gotcha
scope: project              # project | global
volatility: warm            # hot | warm | cold
verified: 2026-05-28
status: active              # active | superseded | contradiction
contradicts:                # slug of conflicting lesson, if any
sources:
  - session:<session-id>
---

**Context:** Setting up the test runner in this repo.
**Symptom:** Suggested `npm test`; user stopped me.
**Root cause:** Repo standardizes on bun; npm lockfile is absent by design.
**Fix:** Use `bun test` for this project.
**Rule:** This user/project uses bun as the package manager and test runner — prefer bun over npm everywhere unless told otherwise.
```

- **Rule vs Fix** is the core discipline borrowed from llm-wiki's `/wiki:ll`: *Fix* is "add X to the codex profile," *Rule* is "each AI tool needs its own profile." The Rule is what generalizes; it's what makes future injection useful beyond the exact original situation.
- The **Rule** is the primary text embedded for semantic search. Context/Symptom/Root cause/Fix are supporting detail.

### Capture Taxonomy (5 categories)

The transcript scan sorts signals into:

1. **error-fix** — an error occurred and was resolved (symptom → root cause → fix).
2. **correction** — the user corrected the assistant's approach, output, or assumption.
3. **discovery** — a non-obvious fact about the codebase, tooling, or domain was learned.
4. **config** — an environment/configuration/setup detail that will matter again.
5. **gotcha** — a surprising pitfall or constraint worth warning future-self about.

### Signal-Noise Guardrails

- **Count check:** a typical session yields **2–7 lessons**. More than ~10 means extraction is too granular — merge or drop.
- **Deduplication:** if multiple events teach the same lesson, emit one merged lesson, not several.
- **No-op is valid:** many sessions produce zero lessons. Capturing nothing is the correct output for a session with no durable signal.

---

## Staleness Model — "evidence with decay"

Distilled lessons are not permanently true. A lesson about a library's API written six months ago may be wrong today. Rather than treat every note as ground truth, each lesson carries:

- `verified: YYYY-MM-DD` — when the lesson was last confirmed accurate.
- `volatility: hot | warm | cold` — how fast this kind of fact decays.
  - **hot** — fast-moving (library versions, API shapes, in-flight decisions).
  - **warm** — moderately stable (project conventions, architecture choices).
  - **cold** — durable (user preferences, hard constraints, domain facts).

At injection time, the `verified` date and `volatility` tier are surfaced **alongside** each note. This converts injection from "treat all notes as ground truth" into "treat notes as evidence with decay" — the assistant can flag when it's leaning on a possibly-stale lesson (e.g. *"per a lesson verified 2025-11-01, tagged hot — may be outdated"*).

We deliberately adopt a *simple* version of llm-wiki's freshness model: just the `verified` date + `volatility` tier. We do **not** implement their full 0–100 composite freshness score in v0.2.

---

## Scope Tiers — project vs global

Lessons have two scopes:

- **project** — tied to one codebase (conventions, architecture, repo-specific gotchas). Written automatically to that project's note store.
- **global** — universal to the user ("I prefer bun over npm", "always run the linter before committing"). Applies across every project.

**Promotion safety rule:** project-scope lessons are written automatically. Global-scope lessons are **never written silently** — a candidate global lesson is surfaced for user confirmation before it's promoted. This prevents project-specific noise from polluting the global scope. (Borrowed from llm-wiki's `--rules` → CLAUDE.md flow, which is user-confirmed, not automated.)

### Global confirmation mechanism

Capture runs async at `SessionEnd`, when no user is present to confirm — so confirmation is necessarily *deferred to the next session*. The v0.2 flow:

1. At capture time, a global-scope candidate is **not** written to the global index. It is appended to a review queue file: `~/.proactive-context/global/pending-lessons.md` (each entry is a full, ready-to-promote lesson block).
2. At the **start of the next session**, the injection/startup hook checks the queue and, if non-empty, injects a single short nudge: *"You have N pending global lessons to review — run `proactive-context lessons review`."* The pending lessons themselves are **not** auto-injected as if active.
3. The user promotes (or discards) via a `lessons review` command — or simply by editing/moving entries in the queue file. Promoted entries move into `~/.proactive-context/global/` and get indexed; discarded entries are deleted.

This keeps the safety property concrete: nothing reaches the global scope without an explicit human action, and the queue can't silently grow into the active index.

---

## Architecture

```
                 ┌─────────────────────────────────────────────┐
                 │              A conversation                  │
                 └─────────────────────────────────────────────┘
   per prompt           │                              │ once, on session end
   (hot path)           ▼                              ▼  (async, off hot path)
        ┌───────────────────────────┐      ┌──────────────────────────────┐
        │  UserPromptSubmit hook     │      │  SessionEnd hook               │
        │  (INJECT — existing)       │      │  (CAPTURE — new)               │
        │  • embed the user prompt   │      │  • read transcript_path JSONL  │
        │    (v0.1 behavior)         │      │  • scan into 5 categories      │
        │  • query project index +   │      │  • distill → 2–7 lessons       │
        │    global lessons (≤2 qs)  │      │    (Category/Context/Symptom/  │
        │  • merge, inject top-K w/  │      │     Root cause/Fix/Rule)       │
        │    verified + volatility   │      │  • stamp verified + volatility │
        └───────────────────────────┘      │  • write project lessons (auto)│
                     │                      │  • queue global candidates     │
                     ▼                      │    for user confirmation       │
        ┌───────────────────────────┐      └──────────────────────────────┘
        │  proactive-context engine  │                     │
        │  (Rust, existing)          │◄────────────────────┘
        │  • sqlite-vec index        │   capture writes lesson .md files,
        │  • semantic query + rerank │   then triggers indexing of them
        └───────────────────────────┘
```

### Components

| Component | Status | Role |
|-----------|--------|------|
| `proactive-context` Rust tool | Existing (v0.1) | Index markdown, semantic `query` + rerank |
| `UserPromptSubmit` hook | Existing | Inject relevant notes/lessons before each prompt |
| `SessionStart` hook | Existing (extend) | Inject `PRODUCT_MODEL.md` as always-on session context |
| `SessionEnd` capture hook | **New (v0.2)** | Distill transcript → structured lesson notes + synthesis pass |
| Product model synthesis | **New (v0.2)** | Merge new lessons into `PRODUCT_MODEL.md`; flag contradictions |
| Contradiction detection | **New (v0.2)** | Link conflicting lessons; surface at injection time |
| Lesson note format | **New (v0.2)** | YAML frontmatter + status/contradicts + Rule/Fix structured body |
| Distillation prompt | **New (v0.2)** | Drives the 5-category scan + count/dedup guardrails |

### Capture flow

1. Hook receives `transcript_path` (JSONL of the full session) from the harness.
2. **Distillation pass:** sends the transcript through the distillation prompt using the taxonomy + guardrails. Output: 0–7 structured lessons, each tagged `category`, `scope`, `volatility`, `verified`, `sources`, `status`.
3. Project-scope lessons are written to the project's lesson store and indexed.
4. Global-scope candidates are queued for user confirmation (not auto-written).
5. **Synthesis pass:** read `PRODUCT_MODEL.md` + new lessons → call the synthesis prompt → write updated `PRODUCT_MODEL.md`. This pass also detects contradictions between new and existing lessons, links them with `contradicts:` fields, and adds them to the Contradictions section of the model.
6. Contradiction detection during synthesis may produce a `pending-reconciliation.md` entry if a contradiction is sufficiently material — injected as a nudge at the next session start.

### Storage

Lesson notes live alongside the v0.1 centralized index:

```
~/.proactive-context/projects/<normalized-path>/
  index.db                    # v0.1 index — now also holds project lessons
  PRODUCT_MODEL.md            # synthesized, always-on project context (injected at session start)
  lessons/                    # per-project distilled lessons (auto-written, indexed into index.db)
    <slug>.md
  pending-reconciliation.md   # material contradictions flagged for user resolution (NOT indexed)
~/.proactive-context/global/
  index.db                    # dedicated global lessons index
  lessons/                    # promoted global lessons (only after user confirmation)
    <slug>.md
  pending-lessons.md          # queued global candidates awaiting review (NOT indexed)
```

**Raw-immutable / distilled-mutable separation** (borrowed): the original transcript is the immutable source; distilled lesson files are the mutable synthesized layer. A future re-distillation pass can rewrite lesson files without touching source transcripts.

### Indexing & Query Topology

How lessons get indexed and searched (resolves the "where does the query run, against what index" question):

- **Project notes + project lessons share one index.** Project-scope lessons are written into the project's own `index.db` (the same v0.1 per-project index). One query covers both ordinary notes and project lessons.
- **Global lessons get a dedicated index** at `~/.proactive-context/global/index.db`. This keeps cross-project knowledge isolated and separately manageable (distinct decay, separate pruning).
- **Injection does at most two queries** — project index + global index — then merges and reranks the combined candidates before taking top-K. (≤2, not 3: project notes and project lessons are one index, not two.)
- **Capture triggers indexing directly.** After writing lesson `.md` files, the capture hook invokes the indexer on those files rather than relying on the filesystem watcher. Rationale: lesson files live in the *central* store (`~/.proactive-context/...`), not inside the watched project source root, so the existing daemon (which watches the project's source dir) would not see them. Direct indexing is more reliable than extending watch paths. (Extending the daemon to also watch the central store is a possible future simplification.)

---

## Configuration (additions to `~/.proactive-context/config.json`)

```jsonc
{
  // ...existing v0.1 fields...
  "capture_enabled": true,
  "capture_model": "anthropic/claude-sonnet-4-6", // distillation is a REASONING task — see note
  "max_lessons_per_session": 7,
  "global_lessons_require_confirmation": true
}
```

**On `capture_model` — this is a quality knob, not a place to default to the cheapest model.** Distillation with the Rule/Fix discipline (separating the generalizable principle from the specific fix, categorizing, deduping) is a *reasoning* task, unlike v0.1's cheap query-decomposition. A small/cheap model (e.g. `gpt-4o-mini`) tends to under-extract or collapse Rule into Fix. Default to a capable mid/high-tier model; the cost is one call per session (off the hot path), so the quality/cost tradeoff strongly favors quality here.

---

## Privacy & Security

- Distillation is an LLM call (OpenRouter) over the transcript — the same opt-in network boundary as v0.1 `generate`. If `capture_enabled` is false, no transcript ever leaves the machine.
- Lessons inherit the v0.1 per-project isolation: project lessons stay in that project's store and never leak into another project's injection. Only **global** lessons cross project boundaries, and only after explicit user confirmation.
- The assistant's own output is *not* indexed verbatim — only distilled lessons are stored, which breaks the self-reinforcing feedback loop.

---

## Current Limitations & Known Trade-offs

- Distillation quality is bounded by the `capture_model` and the prompt; a poor extraction is "what you get" until re-distilled (v0.2 is one-pass).
- No automated contradiction resolution between an old lesson and a newer one beyond the volatility/verified signal surfaced at injection time.
- Global-scope confirmation requires a UX surface (where/how the user confirms); v0.2 may start with a simple queued-file-for-review approach.
- Adds an LLM call per session (cost + a few seconds), though it runs off the hot path.

---

## Open Questions (need a decision before/around implementation)

- **Transcript-enriched injection queries — DECIDED (v0.3): injection cadence (a), per-prompt generate.** Earlier this was an open question with a recommendation to ship bare-prompt and defer enrichment. That recommendation is **superseded.** The chosen design: on *every* user prompt, injection folds in the **last N turns** of conversation (read + parsed from `transcript_path`, default N = 6, `0` disables) by **concatenation** (not summarization — summarizing would add an LLM call to the hot path), hard-capped in length to bound the embed cost. Beyond enriched retrieval, injection no longer dumps raw query results: it runs the generate pipeline (fan-out → retrieval → optional rerank → optional read_file → LLM compile) to produce a **tight, relevance-filtered briefing**, with `PRODUCT_MODEL.md` as an *input* (never injected verbatim). Because this is on the hot path, it runs under a strict latency/token budget (faster `inject_model`, fan-out/prefetch caps, a hard `inject_timeout_ms` timeout) with a free, non-blocking fallback: the cheap raw-query result is computed first and emitted if the compile times out or fails, so a slow/failed generate never blocks the prompt. The replacement of the TypeScript hook with a Rust `inject` subcommand and the full budget/fallback design are specified in `tail-system.md`.
- **Confirmation UX surface.** v0.2 commits to a queue file + next-session nudge + `lessons review` command. Is a richer surface (e.g. an interactive confirm prompt) wanted, or is the minimal file-based flow enough for v1?

## Future Directions (Not Yet Prioritized)

- **Two-pass / deferred synthesis:** capture raw structured lessons fast, re-synthesize periodically as the distillation prompt improves (llm-wiki's `compile` pattern).
- Full 0–100 composite freshness scoring with decay curves per volatility tier.
- Automated stale-lesson flagging / archival rather than indefinite retention.
- Active contradiction detection (new lesson supersedes old → archive the old).
- A `lessons` subcommand to list/search/prune the lesson store.
- Promotion of confirmed global lessons directly into `CLAUDE.md`.

---

## Non-Goals (v0.2 scope)

- Raw conversation indexing (explicitly rejected — distill don't dump).
- Real-time / mid-conversation capture (capture is post-session only).
- Indexing assistant output verbatim.
- Full freshness scoring and deferred two-pass synthesis (deferred to future).
- A confirmation UI beyond a minimal review surface for global candidates.

---

## Success Metrics (Intuitive)

- **The user never has to say the same thing twice.** A preference stated, a correction given, a rule violated and caught — each happens at most once. After that, the system knows.
- The assistant begins a session already knowing how this user wants this product built. No re-briefing. No re-explaining conventions.
- Rejected approaches stay rejected — the assistant does not re-propose what the user has already overridden.
- Contradictions are visible, not hidden. The user encounters them explicitly at reconciliation time, not as silent drift.
- Stale lessons are flagged, not silently trusted. The user isn't misled by outdated guidance.
- Capture produces a tight set of real lessons (2–7), not a noisy pile.
- `PRODUCT_MODEL.md` reads like a concise, accurate living document of the project's conventions — not a pile of raw notes.
- Global scope stays clean — no project-specific noise leaks across projects.

---

## Summary

v0.2 closes the loop: v0.1 surfaces what the user *wrote*; v0.2 captures what the user and assistant *figured out together* and feeds it forward permanently.

The core bet: LLM tokens are infinitely cheaper than human time. Every correction is a learning event; the system's job is to ensure it only happens once. Capture distills transcripts into structured, scoped, decay-aware lessons and synthesizes them into a growing product model; injection resurfaces them proactively on every future prompt. Contradictions are never silently discarded — they are preserved, surfaced, and resolved with the user's explicit input.

The discipline that keeps it high-signal: distill don't dump, Rule vs Fix, tight counts, always-on product model, contradiction preservation, and confirmed global promotion. Each constraint exists because noise is the death of a memory system — and the goal is a system the user can trust completely, not one they have to second-guess.
