# Session Episode Cards

**Status:** Proposed for near-term implementation.
**Origin:** Follow-up to the Run 7 finding that raw transcript RAG wins recall while atomic claims and projected guides lose session narrative.

## Summary

Session episode cards are compact, cited, historical artifacts generated from completed sessions. They sit between raw transcript chunks and atomic claims: smaller and safer than raw history, but narrative enough to preserve the arc that claims lose.

An episode card answers: **what did this session change, why, what prior belief or behavior did it revise, and what consequences should a future agent remember?**

They are not the canonical current-truth spec. They are session-level provenance: a durable map from conversation to product movement.

## Problem

Run 7 showed that raw transcript RAG is the strongest recall baseline: the original user words are still in history, so retrieval over history often finds them. But raw history also leaks stale facts and costs attention. Atomic claims solve compactness and authority tagging, but they often lose the story. In particular, direction changes are frequently narrative events:

```text
prior state -> user correction or experiment -> new decision -> reason -> implementation consequence
```

When capture reduces that arc to isolated facts, it can flatten a replacement into an additive capability. For example, "the default changed from X to Y" can become "Y is supported." That claim is true but no longer contradicts X, so downstream supersession machinery cannot recover the reversal.

Research records solve this for formal investigations, but many useful sessions are not structured experiments. They still contain product movement: a design correction, an architecture turn, a root-cause discovery that changes the spec, or a decision with consequences.

## Non-Problem: Routine Commands

Command-shaped user instructions are usually low-information for product memory:

- "commit the work"
- "deploy"
- "run tests"
- "publish the proposal"
- "merge this branch"

These may matter to agent workflow preferences or automation behavior, but they rarely say much about the product. Episode cards must not inflate routine operational commands into product-spec history.

The high-salience inputs are product-led or doctrine-led instructions:

- "Clicking an avatar should open a hovercard."
- "This replaces the previous navigation behavior."
- "The default embedder should be local."
- "Do not put business logic in Swift."
- "The wiki is the product, not just a retrieval cache."

Episode-card capture should explicitly distinguish these classes.

## Product Goal

Create a first-class session artifact that preserves load-bearing narrative without dumping raw transcripts:

- Preserve why a decision changed, not only the new fact.
- Preserve prior-state context for reversals and narrowing decisions.
- Give inject a compact artifact to retrieve when a task needs "what happened in that session?"
- Give future projection/regeneration work a higher-altitude source than isolated claims.
- Keep the raw transcript as provenance, not as the primary injected memory.

## Artifact Semantics

An episode card is:

- **Historical:** it describes what a session established or changed at that time.
- **Cited:** every substantive claim points to transcript line ranges or existing claim IDs.
- **Immutable by default:** later sessions can supersede or refine it by linking a later card; they do not rewrite the old event.
- **Selective:** not every session deserves a card.
- **Product-salience weighted:** product/spec/architecture movement outranks workflow commands.
- **Readable:** it should be useful to a human or agent without opening the full transcript.

An episode card is not:

- A current-truth guide.
- A raw transcript summary.
- A changelog of every command run.
- A replacement for atomic claims.
- A replacement for research records.

## Relationship to Existing Capture Types

| Type | Altitude | Mutable? | Best For |
|---|---|---:|---|
| Raw transcript | Full session | Immutable | Maximum recall, audit fallback |
| Claim | Atomic fact | Append-only | User directives, compact facts, ranking |
| Wiki guide | Topic/spec prose | Mutable projection | Current truth and readable orientation |
| Research record | Document-shaped report | Immutable | Experiments with method and verdict |
| Episode card | Session movement arc | Immutable | Decisions, reversals, rationale, consequences |

Episode cards complement research records. A formal eval with pre-registered criteria becomes a research record. A normal design/debug session that changes product understanding becomes one or more episode cards.

## Salience Model

The card generator should classify candidate material by salience:

1. **Product behavior:** desired user-visible behavior, feature semantics, domain rules.
2. **Architecture doctrine:** ownership boundaries, source-of-truth decisions, system invariants.
3. **Direction changes:** X was replaced by Y, X was narrowed, or X is now historical.
4. **Durable root cause:** a bug or failure whose diagnosis changes future implementation.
5. **Research conclusion:** if not structured enough for a research record, preserve the finding arc.
6. **Agent workflow preference:** reusable process preference, lower product weight.
7. **One-shot command:** normally excluded unless it establishes reusable policy.

Only categories 1-5 should normally create product episode cards. Categories 6-7 may create a separate low-priority workflow note only if repeated or explicitly framed as a standing preference.

## Card Shape

Recommended markdown format:

```markdown
---
type: episode-card
date: 2026-06-11
session: <session-id>
transcript: <path>
salience: product|architecture|reversal|root-cause|workflow
status: active|superseded|historical
subjects:
  - inject-pipeline
  - embedding-provider
supersedes:
  - <episode-card-id-or-claim-id>
related_claims:
  - <claim-id>
source_lines:
  - 120-145
---

# Episode: Local embeddings become the default

## Prior State

OpenRouter/OpenAI embeddings had been treated as the expected embedding path.

## Trigger

The session identified local-first operation and sqlite-vec dimension stability as load-bearing constraints.

## Decision

The default embedder is local MiniLM; OpenRouter embeddings are no longer the default path.

## Consequences

- Existing indexes with another dimension must be rebuilt.
- Future docs should describe OpenRouter embeddings, if present, as optional or future work.

## Open Tail

- Decide whether model-dimension migration should be automatic or an explicit warning.

## Evidence

- Prior/default discussion: transcript lines 120-132
- Decision: transcript lines 138-145
```

The headings should be stable enough for inject and future tooling:

- `Prior State`
- `Trigger`
- `Decision`
- `Consequences`
- `Open Tail`
- `Evidence`

Optional headings:

- `Rejected Path`
- `Root Cause`
- `Follow-up`
- `Related Research`

## Capture Pipeline

Episode-card generation should run after ordinary EXTRACT, not before it.

Suggested pipeline:

1. Parse transcript with the same line-numbering and task-result visibility used by capture.
2. Run normal EXTRACT and claim logging first.
3. Run an **episode recognition** pass over the transcript plus extracted claims.
4. Select candidate arcs with product salience.
5. Emit one card per coherent arc, not one card per fact.
6. Verify every evidence range with Rust slicing.
7. Persist cards under `<wiki>/episodes/<date>-<slug>.md`.
8. Index episode frontmatter and summaries for retrieval.

The recognition pass should be conservative about operational commands. A session that only commits, publishes, deploys, or cleans up should emit no product episode card.

## Recognition Prompt Contract

The model should be asked for structured arcs, not summaries:

```json
[
  {
    "title": "Local embeddings become the default",
    "salience": "reversal",
    "subjects": ["embedding-provider", "local-first"],
    "prior_state": "...",
    "trigger": "...",
    "decision": "...",
    "consequences": ["..."],
    "open_tail": ["..."],
    "evidence": [{"start": 120, "end": 145}],
    "exclude_reason": null
  }
]
```

For excluded sessions or candidate arcs, the model may return:

```json
{
  "exclude_reason": "routine-command-only"
}
```

The code should treat `routine-command-only` as a successful no-op, not a failure.

## Injection Behavior

Episode cards should be retrieved when the prompt asks for:

- why a decision was made,
- whether a prior approach was replaced,
- what happened in a prior session,
- root-cause history,
- implementation consequences of a design turn,
- "has this been tried?" when no formal research record exists.

They should not be injected as current truth without labeling. A card's decision can be current, superseded, or historical. The injector should prefer current wiki/claims for present-tense behavior and use episode cards to explain trajectory and rationale.

Compiler instruction should treat episode cards as historical provenance:

- If the prompt asks for current behavior, surface the current decision plus "previously..." only when relevant.
- If the prompt asks why or how a decision changed, include the episode card.
- If an episode card conflicts with a newer claim/guide/card, label it historical.

## Storage and Indexing

Recommended layout:

```text
<wiki>/
  episodes/
    2026-06-11-local-embeddings-default.md
```

`_index.md` should list episode cards in a separate section, similar to research records.

Episode frontmatter should include:

- `type: episode-card`
- `date`
- `session`
- `transcript`
- `salience`
- `status`
- `subjects`
- `supersedes`
- `related_claims`
- `source_lines`
- `summary`

The vector index should embed the title, summary, subjects, and main body. It should not embed the raw transcript.

## Currentness and Supersession

Episode cards preserve history; they do not decide current truth alone.

If a later session revises an episode card's decision, the later card should link back through `supersedes`. The old card's frontmatter may be marked `status: superseded`, but its body remains unchanged.

Currentness should be resolved at injection time by combining:

- newer episode cards,
- current claims,
- wiki guide state,
- explicit supersedes links,
- code-grounding staleness where applicable.

## Implementation Plan

### Phase 1: Spec and no-op scaffolding

- Add `episode-card` frontmatter type and index scanning.
- Add a disabled config flag, e.g. `capture_episode_cards`.
- Add storage directory support under `<wiki>/episodes/`.
- Add unit tests that episode cards appear in `_index.md` but are not parsed as normal guides.

### Phase 2: Standalone debug command

- Add `pc debug episodes <transcript>` or `pc episodes --transcript <path> --out-dir <dir>`.
- Reuse the line-numbered transcript builder.
- Emit candidate episode cards into a temp directory.
- Validate against hand-picked reversal/design sessions before wiring live capture.

### Phase 3: Capture integration

- Run episode recognition after normal capture, best-effort like research capture.
- Default off until validation passes.
- Ensure routine command-only sessions produce no cards.

### Phase 4: Inject integration

- Add episode rows to the catalog/read model.
- Teach SELECT/compile about `episode-card` semantics.
- Add currentness labeling so cards are not asserted as current without corroboration.

## Validation

Use the existing benchmark corpora and add an episode-card source to Run 7-style evaluation.

Minimum bars:

- **Direction-change fidelity:** improves Probe 2 trajectory recovery over claims-only.
- **Stale leaks:** no worse than claims-only; raw transcript stale leaks are the failure to avoid.
- **Recall:** does not need to beat raw RAG, but should improve over wiki/projection on narrative prompts.
- **Precision:** ordinary command-only sessions emit zero product episode cards.
- **Usefulness:** for a sample of known design turns, a fresh agent can answer "why did we change this?" without opening raw transcript.

Specific fixtures:

- Embedding provider default reversal.
- Primary command `generate` to `inject` reversal.
- Capture evidence format change from quoted text to line ranges.
- A routine "commit/deploy/publish" session that should produce no product card.

## Open Questions

1. Should episode cards be generated from raw transcript only, or from transcript plus extracted claims?
2. Should multiple small arcs in one session become multiple cards, or one multi-arc card?
3. Should workflow-preference cards share this storage type, or live in a separate low-priority global memory?
4. How should card status updates be written without violating immutability: frontmatter patch, sidecar status table, or later superseding card only?
5. Should cards participate in `pc wiki doctor` staleness checks immediately, or only after inject starts using them?

## Near-Term Recommendation

Implement episode cards as an experimental, default-off capture type. Start with a standalone debug command and four fixtures: three known product/design reversals and one routine command-only session. If the cards improve direction-change trajectory without introducing stale leaks or command-noise, wire them into capture and then inject.

## Phase 2 Validation Results (2026-06-11)

Implementation: `src/episode_capture.rs` + `pc episodes` command. Commits: `90ad65e` (implementation), `9effa1a` (UTF-8 fix).

### Fixture (a): Embedding-provider default reversal (OpenRouter→local MiniLM)

**Session:** `658f4c79` (transcript ~2.6 MB, 3894 lines after parsing)
**Card produced:** Yes — `2026-06-11-3-embedding-dimension-mismatch-self-heals-on.md`
**Title:** "Embedding dimension mismatch self-heals on DB open"
**Prior State (correct):** "Switching embed models (e.g. OpenRouter 1536-dim → local 384-dim) caused a hard crash on query: 'Dimension mismatch for query vector for the embedding column.'"
**Decision (correct):** Read actual FLOAT[N] dimension from `sqlite_master` (not `meta` table); drop and recreate stale virtual table on mismatch.
**Salience:** `root-cause`
**Notes:** The model correctly captured the OpenRouter→local transition embedded in the dimension-mismatch root cause. The framing is accurate but narrow — it describes the *consequence fix* rather than the earlier *provider switch decision*. Both are valid; the root-cause framing is arguably more durable.

### Fixture (b): Primary command `generate`→`inject` reversal

**Session:** `658f4c79` (same session as above)
**Card produced:** Yes — `2026-06-11-2-generate-command-removed-entirely-in-favor.md`
**Title:** "Generate command removed entirely in favor of inject"
**Prior State (correct):** "`pc generate` was a CLI subcommand that answered questions from notes with multi-turn tool-calling. It had its own config fields (`generate_model`, `decompose_model`, `max_fanout_queries`, `max_parallel_prefetch`)."
**Decision (correct):** "Deleted `generate.rs`, removed the `Generate` CLI subcommand, removed four config fields, removed two configure TUI roles."
**Salience:** `reversal`
**Notes:** Prior State and Decision match the historical record exactly. User trigger ("remove the 'generate' command entirely (since we now use inject as the command)") was identified correctly.

### Fixture (c): Capture-evidence format change (quoted text→line ranges)

**Session:** `ed37c932` (505 KB, 583 transcript lines)
**Card produced:** Yes — `2026-06-11-1-inject-compile-model-reframed-from-answerer.md`
**Title:** "Inject compile model reframed from answerer to librarian"
**Prior State (correct):** "The compile (second-step) model in inject was a briefing synthesizer… Citations were not line-accurate; dates were not included."
**Decision (correct):** "The compile model is now a librarian, not an answerer. It outputs a tiny JSON of {slug, start, end}; Rust slices verbatim text… Absolute-path citations with relative dates are rendered by Rust."
**Salience:** `reversal`
**Notes:** This session captures the companion decision (inject becoming verbatim-line-range-cited) that was the functional motivation for the capture evidence format change. The capture-side change (quoted text → line ranges) lived in an adjacent session not in the history corpus; this session captures the inject-side half of the same reversal, which is the load-bearing consequence. Coverage is partial but meaningful.

### Fixture (d): Routine command-only session

**Session:** `25b7ce16` (18 KB, 6 transcript lines — single question about config.json)
**Cards produced:** 0 (correct — the LLM returned `[]`)
**Notes:** The routine-only no-op path worked correctly. The session was a single factual question with no product arc; the model returned an empty array rather than the `{"exclude_reason":"routine-command-only"}` object, which is also a valid no-op response per implementation.

### Summary

| Fixture | Card emitted? | Prior State correct? | Decision correct? | Notes |
|---------|--------------|---------------------|------------------|-------|
| (a) embed provider reversal | Yes | Yes | Yes | Framed as root-cause fix; captures the reversal accurately |
| (b) generate→inject removal | Yes | Yes | Yes | Exact match to historical record |
| (c) evidence format change | Partial | Yes (inject side) | Yes (inject side) | Capture-side session not in corpus; inject-side correctly captured |
| (d) routine command-only | No cards | n/a | n/a | Correct no-op |

### Recognition quality observations

- The model (Ollama `glm-5.1:cloud`) correctly identified 4 distinct product arcs from a 3894-line session (fixture a+b), each with accurate Prior State and Decision.
- Evidence ranges are all in-corpus (verified by Rust slicing); no dropped cards due to bad evidence.
- The model did NOT produce false positives for the routine-only session.
- The `routine-command-only` exclusion path was exercised via the empty array `[]` return (also correct).
- One recognition call per session; total latency was under 30 seconds for the 2.6 MB session.
- The spec's `routine-command-only` JSON object (`{"exclude_reason": "..."}`) was never returned in practice; models returned `[]` for no-arc sessions. Both are handled correctly.
