# Research Capture: Investigation Artifacts as a First-Class Knowledge Type

**Status:** Proposed (v0.6).
**Origin:** The claims-first validation sessions (2026-06-10/11) — an agent ran a five-run empirical investigation, and the only reason its findings survive is that they were hand-distilled into `claims-first-learnings.md`. pc's own pipeline, run over those sessions, would have shredded them. This spec exists to close that gap.

## 1. The problem

Some sessions don't produce direction — they produce **research**. An agent runs an experiment, hunts a bug to root cause, explores an architecture, benchmarks alternatives. The knowledge such a session yields is document-shaped:

```
question → method → pre-registered criteria → evidence → finding → diagnosis → revision
```

The capture pipeline's unit is the atomic cited claim, and atomic extraction destroys this knowledge *while preserving every fact in it*. "Store A preserved trajectory 7/8," "the miner passed content where a path was expected," "p95 dropped 51%" — each true, each citable, each routable somewhere. And the thing that mattered is gone, because the value was never in any single assertion. It was in the structure: that criteria were written before the run, that the result forced a diagnosis, that the diagnosis revised a belief ("we thought breadcrumbs were fragile; we measured; we were wrong"). A retraction arc is a direction change with its evidence attached — exactly the signal this project holds most precious — and no pile of atoms reassembles it. This is the altitude problem again: atomic extraction has no vantage point above a single line, and research findings live at document altitude.

The cost of losing it is concrete and recurring: settled empirical questions get re-litigated, and expensive experiments get re-run. The next agent that proposes "why not store claims instead of wiki pages?" should be told *"tested 2026-06-10 across two corpora; split verdict; supersession linking was the decisive gap; see Run 5"* — and it will only accept that if the finding arrives **with its method attached**. A naked conclusion invites argument; a conclusion with pre-registered criteria and a preserved evidence chain ends one.

## 2. The key insight: the distillation already exists in the transcript

Investigation sessions are the one class of session whose *deliverable is prose*. The agent already wrote the structured report — it is sitting in the transcript, complete, in the agent's own words, at the right altitude. So research capture is **not** an extraction or synthesis problem:

> **Capture's job for this knowledge class is recognition + provenance: notice that the session produced an investigation artifact, slice it whole and verbatim (the existing line-range citation machinery), date it, and persist it as a unit.**

No new generation step. No re-synthesis that could mangle what it touches. This is *cheaper* than what EXTRACT does today, and it is maximally faithful to the show-your-work thesis: the artifact is the agent's actual words, cited to the exact transcript lines, by construction.

## 3. Requirements

**R1 — A third capture type, with lab-notebook semantics.** Alongside direction claims (atomic, evolving, current-truth-with-history) and entities/definitions (entity spec v0.5), capture recognizes **research records**: document-shaped, dated, and **immutable**. An experiment happened; its result is never reconciled, revised, or edited. A later investigation may *supersede* it via an explicit link — never by rewriting it. Research records are append-only by nature; keep-everything holds structurally, not behaviorally.

**R2 — Recognition, not synthesis.** Capture detects that a session contains investigation products and locates the artifact's span(s) in the transcript. Detection signals, in rough reliability order: a structured report emitted by an agent or subagent (headings, tables, verdict language); pre-registered criteria or an explicit method stated before results; experiment/benchmark execution visible in the session; user framing ("run the test", "let's see what happens", "validate this"). The artifact is persisted as a verbatim slice with line-range provenance — the same integrity-by-construction mechanism as claims: the model points at lines; Rust slices the text; a fabricated research record is unrepresentable.

**R3 — Subagent reports are in scope.** For investigation sessions, the sidechains *are* the research. Current capture paths filter sidechain content; research recognition must at minimum read subagent final reports where they surface in the main transcript (task results), and the sidechain filter should be revisited for sessions classified as investigations. A research record notes which agent produced it.

**R4 — Inject delivers findings with method attached.** When a prompt touches a question a research record settles, inject surfaces the finding *plus* enough of the method/criteria to make it load-bearing ("measured, pre-registered, n=, caveats"), and a pointer to the full record. Ranking: research records answer "has this been tried/tested?" prompts and design-debate prompts; they should outrank topical claims there and stay out of the way elsewhere.

**R5 — Findings age; records don't.** The record is immutable, but its findings can be invalidated by code change or by later experiments. Staleness handling follows the demote-not-delete design: a superseding record links back ("Run 5 revises Run 4's diagnosis"); the code-grounding detector applies (a record citing deleted machinery gets flagged); nothing is ever rewritten. The chain of records *is* the trajectory of what the project has learned — the same X→Y signal as direction changes, at investigation altitude.

**R6 — Topical guides link to research records, not the reverse.** Guides carry current truth; where a guide's statement rests on an experiment, it cites the research record the way it cites transcript evidence today. Doctor verifies the links; the record itself is never edited to match the guide.

**R7 — Recognition precision over recall, for this type only.** Unlike claims (recall-biased), research capture must not classify every verbose agent summary as "research" — a flood of pseudo-records would bury the real ones and erode trust in the type. Gate on the strong signals (structured report + method/criteria present). A missed research artifact still degrades gracefully: its facts are captured as ordinary claims.

## 4. Validation — already paid for

The claims-first investigation provides a ready-made gold standard:

1. Run research-capture over the 2026-06-10/11 sessions (this conversation and its subagent reports).
2. Diff the captured record(s) against the hand-written `claims-first-learnings.md` — the human-judgment distillation of the same material.
3. The diff is the gap measurement: what did recognition miss, what did it wrongly include, did provenance survive. Same temporal-holdout spirit as the eval harness, zero new corpus work.

A second, cheaper smoke test: any session where a subagent returned a structured report (bug root-cause, architecture review) — the record should be that report, sliced whole, and nothing else.

## 5. Relation to existing designs

- **Entity spec v0.5 (R3/R5):** this spec elevates R5's "deep-research guides" from an attachment on entities to a first-class capture type with its own semantics. R3's in-session-investigation sourcing is the same mechanism at definition scale.
- **Citation-anchored capture:** supplies the slicing/integrity machinery unchanged.
- **Claims-first (open):** orthogonal — research records are immutable documents regardless of whether topical knowledge lives in guides or a claim log. Both stores would link to the same records.
- **`claims-first-learnings.md`:** the concrete exemplar of target output — written by hand precisely because this spec wasn't implemented.

## 6. Open questions

1. **Granularity:** persist the whole report as one record, or also index individual findings (F1…Fn) for retrieval? (Leaning: whole record is the unit of truth; findings get embedded for retrieval but always resolve to the record.)
2. **Storage shape:** `docs/wiki/research/<date>-<slug>.md` vs. flat-with-type-frontmatter — same D1-style decision as topics; immutability should be enforced by doctor checks either way.
3. **Multi-session investigations:** the claims-first work spanned five runs across two days. One record per run, or one evolving... no — immutability says one record per run plus a thread link. Confirm this composes.
4. **Recognition false-positive rate in the wild:** R7's gating needs measurement on ordinary (non-investigation) sessions.
