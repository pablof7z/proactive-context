# proactive-context — Citation-Anchored Capture & the `wiki_*` Tool Contract

**Status:** Proposed (v0.4)

**One-line description:**
Capture is reframed from "extract lessons so the assistant stops repeating mistakes" into "continuously reverse-engineer the complete, organized product specification from the conversation, losing no human-supplied nuance." The wiki becomes a *living, regenerable spec*; every assertion in it is anchored to a verbatim passage of the conversation — sliced by Rust from the transcript line ranges the model cited — that justifies it.

> **Meta note:** This document was written by hand, but it is deliberately structured as the artifact the system is meant to produce automatically — resolved decisions plus the rationale that justifies them, with the conversation that produced them cited underneath. It is its own worked example.

---

## Problem

The v0.2/v0.3 capture pipeline (`distill_lessons` → `plan_wiki_ops` → `apply_wiki_ops`) has three structural limits:

1. **Defensive framing.** Its categories — correction, error-fix, gotcha, config, preference — are all about *the assistant's behavior* or technical facts. There is no first-class notion of *the product gaining or revealing a capability*. When the user says "clicking an avatar should open the user's profile," that is a **product-spec fact**, not a correction of the assistant, and it falls through the cracks.

2. **Append-only enrichment.** The wiki-planning prompt enriches guides append-only ("never full rewrite"). But product facts *change* — "navigate to profile" becomes "open a hovercard." Append-only means the wiki accretes stale and contradictory statements that get retrieved as if current. (This also contradicts the project's own design notes, which already say capture "HAS the authoritative source, so it can update guides aggressively.")

3. **No provenance, so no defense against hallucination.** The model emits free-form JSON we parse and trust. Nothing ties an assertion to evidence, so a confidently-worded but invented requirement is indistinguishable from a real one.

---

## The reframe: the wiki is a regenerable spec, not a changelog

**North-star test:** *If you dropped this wiki — and nothing else — onto a fresh project and asked a model to one-shot the app, the result should be the product, at full nuance.* The wiki is a complete, organized specification of how this user wants this product to work. That test is also the **eval**: run it against the validation harness, diff the regenerated app against the real one, and the gaps are exactly the nuance capture dropped.

This changes the unit of capture. We store **positive specifications of desired state**, not events:

- ❌ "avatar is broken" (an event, frozen, becomes false the moment it's fixed)
- ✅ "On the feed, tapping an avatar opens a hovercard with the user's details" (a spec, durable, the implementation catches up to it)

Storing desired state also dissolves most of the supersession problem: the day the avatar starts working there is nothing to update, because the spec was always "should open a hovercard." Real overrides only happen when the *spec itself* reverses, which is rarer and is genuine spec evolution the wiki should track.

**Governing principle:** *Human time is irreplaceable; tokens are buyable.* The cost asymmetry is stark — a dropped nuance costs the user irreplaceable time re-explaining it; an over-captured nuance costs a few tokens and some wiki bulk the merge step manages. So capture is biased toward **recall**: when in doubt, capture. The triage gate's `NO` therefore narrows to two cases only — *purely transient operations* ("git pull, the remote moved") and *already-specified* (the wiki already knows this).

---

## The citation model

Every spec statement is anchored to the conversation passage that justifies it. This is event-sourcing applied to the spec:

- The **spec statement** is a *mutable projection* — it gets rewritten freely as understanding sharpens.
- The **citation log** is the *immutable event stream* — append-only, the ground truth that the projection is derived from.

### Citation IDs

Format: `[^<5-char-session-prefix>-<n>]`, e.g. `[^a3f9c5-2]` = the 2nd citation minted from session `a3f9c5…`.

The session prefix makes IDs **race-free by construction** — capture runs as concurrent background processes (see *Concurrency*), and a global monotonic counter would collide. Session-anchored IDs need no coordination and embed provenance in the ID itself. The `-n` suffix is per-session and trivially serialized within a single capture run.

### The citation log

One append-only file per project wiki: `_citations.log`. Never embedded, never injected, never retrieved — **provenance and audit only.** (Putting raw conversation into the retrieval index is the precise failure — indexed contradictions, staleness, feedback loops — that *distill, don't dump* exists to prevent.)

Each entry:

```
a3f9c5-2 | 2026-05-29T14:32:10Z | session:a3f9c5-3cb7-… | <verbatim text Rust sliced from the cited transcript line ranges>
```

### `evidence`: line ranges into the transcript, not a quoted string

The evidence for a statement is **line ranges into the session transcript**, not text the model types. The capture agent is shown the transcript **line-numbered** (exactly as the inject librarian is shown line-numbered guides), and each mutating call carries `evidence: [{ start, end }]` — the line ranges that justify the statement. **Rust slices the verbatim text out of those lines**; the model never reproduces a single character of the conversation.

This is the same *model→ranges, Rust→slice* pattern the injection side already uses to cite guide excerpts, applied in reverse: inject selects ranges from guides; capture selects ranges from the transcript.

**The single governing instruction** is unchanged in spirit:

> `evidence` must make the decision self-justifying to someone who wasn't there — when citing an approval, include the lines of the proposal it approved; when citing a correction, include the lines being corrected.

This makes the "the user just said *yes*" problem disappear without any schema. A bare affirmation is meaningless *alone* — but a range that spans the exchange never isolates it:

```
312| Assistant: avatars should open a hovercard instead of navigating?
313| Pablo: yeah do that
```

`evidence: [{ start: 312, end: 313 }]` carries the proposal (the content) and the "yes" (the authorization) together. A correction grabs the line being corrected plus the correction. Cross-turn evidence (a proposal and a non-adjacent affirmation) is simply **multiple ranges** — no elision marker, no quoting.

### Why ranges, not a verified quote (the anti-hallucination gate)

An earlier draft had the model emit a free-form `relevant_transcript` *string* and had Rust verify it occurred in the transcript (whitespace-normalized substring; reject on miss). Ranges are strictly better:

- **Integrity is truly *by construction*, not *by verification*.** With a quoted string, a fluent model can invent a passage and Rust must catch it — and any matcher loose enough to tolerate the model lightly reflowing a real quote (the realistic failure) is also loose enough to admit a near-miss fabrication. With ranges there is **nothing to verify**: Rust extracts actual transcript lines, so a citation cannot be anything other than text that was really said. A citation is no longer "Rust confirmed this *matches* something said" but "this *is* what was said, sliced out by Rust."
- **No matching-tolerance dial to tune.** Substring-vs-fuzzy threshold tuning (which fails differently for prose vs code blocks) disappears entirely.

The model still cannot assert anything without evidence (ranges are required) and cannot fabricate evidence (it supplies indices, not text) — so it cannot author a requirement the human never gave.

> **Fallback if evidence is kept as a string instead of ranges:** aggressive normalization (collapse whitespace, straighten smart quotes/dashes) + strict substring → on miss, **reject with feedback and let the agent re-quote verbatim** → and only as a last resort, high-threshold fuzzy matching that is **explicitly marked as fuzzy-verified** so the weaker provenance is visible, never silent. Ranges avoid all of this and are the chosen design.

### Markers in injected context

The `[^id]` markers live in the guide prose. When the injection librarian slices a guide excerpt, those markers ride along into Claude Code's context and are **deliberately left intact, not stripped.** Rationale: they are a feature, not noise — Claude can follow a marker into `_citations.log` (a deliberate, on-demand file read — *not* indexed retrieval, so *distill, don't dump* still holds) to see the verbatim exchange that justifies a statement.

Inject therefore **conditionally prepends a one-line preamble** to the injected block — only when the body actually contains a `[^` marker:

> *Inline `[^id]` markers cite verbatim source-conversation evidence in `<wiki>/_citations.log`; read it to see why a statement exists.*

No marker present (e.g. before any citation-anchored capture has run) → no preamble. This keeps the injection-side `render_selection` unchanged (it already slices verbatim) and adds zero noise until citations exist.

---

## The `wiki_*` tool contract

Capture becomes a **tool-using agent loop**, not a JSON-emitting pipeline. The model decides *what* changed semantically; Rust owns *how* it is persisted and every invariant. The model never types a `[^id]` character.

**Addressing:** statements are addressed by their **section heading** within a guide — the heading is the stable anchor. There is no separate statement-ID registry (deliberately — see *Rejected approaches*). `revise`/`remove` operate on a section; Rust carries forward the `[^id]` markers already present in that section and mints new ones for new evidence.

### Read tools (no side effects)

```
wiki_list()                  → [{ slug, title, summary }]   (the index)
wiki_read(slug)              → guide body, WITH section headings and existing [^id] markers visible
```

### Mutating tools (each requires evidence; Rust enforces integrity)

```
wiki_create({ slug, title, summary, sections: [{ heading, text, evidence }], tags?, volatility? })

wiki_add_statement({ slug, section, text, evidence })

wiki_revise_statement({ slug, section, text, evidence })
   → Rust replaces the section's prose, preserves that section's prior [^id]s,
     mints a new id for `evidence`, appends the sliced text to _citations.log.

wiki_remove_statement({ slug, section, evidence })
   → removal is itself a cited event (evidence = the lines showing the decision to remove).
```

`evidence` — line ranges `[{ start, end }]` into the line-numbered transcript — is **required** on every mutating call. Rust:
1. Slices the verbatim text from those transcript line ranges (nothing to verify — the slice *is* the evidence; there are no out-of-transcript ranges to fabricate).
2. Mints the citation id, writes the `[^id]` marker into the prose at the asserted statement.
3. Appends the sliced text to `_citations.log`.

### Rust-owned structural maintenance (the "Structural Guardian")

The model is **not** trusted with graph or index invariants. After any mutating call, Rust automatically:

- maintains **bidirectional** `See Also` links (write a link A→B, Rust writes B→A);
- rebuilds `_index.md`;
- re-embeds the changed guides into `index.db`.

A `wiki_link(a, b)` tool exists, but link/index/embed correctness is Rust's responsibility, not the model's.

---

## Named invariants

These belong in the doc because they are the load-bearing rules a future change must not break:

1. **Distill, don't dump.** Only synthesized spec statements are retrievable. Raw conversation lives only in `_citations.log`, which is never indexed.
2. **Integrity by construction.** No mutating tool call can assert anything without `evidence` (transcript line ranges), and Rust slices the cited text verbatim from those lines — the model supplies indices, never text. Uncited and fabricated assertions are *unreachable states* by construction, not states we verify against after the fact.
3. **The citation log is the only append-only structure.** Everything pointing into it — spec statements, sections, whole guides — is fully mutable, including deletion. The log only ever grows.
4. **Positive specification.** Statements describe desired state, not events. "Should open a hovercard," never "is broken."
5. **Affirmation is captured as context, not authorization-as-evidence.** Because the `evidence` range includes the proposal an approval refers to, a bare "yes" is never the evidence on its own. A *qualified* yes ("yes, but make it optimistic locking") is a **revision** — the clause after "but" is user-authored content that may override the proposal, not an affirmation.

> **Caveat we do not oversell:** these guarantee every assertion is *cited and real*, not that it is *correct*. A bad restatement is still possible — but the cited evidence in `_citations.log` (sliced verbatim from the transcript) sits next to it as ground truth, so the regeneration eval (and the user) can catch a drifted spec by reading what was actually said.

---

## What this replaces, and what stays

**Replaced** — the tool loop subsumes all three of these into one agentic capture pass:
- `distill_lessons` (the Sonnet "extract lessons" call)
- `plan_wiki_ops` (the Sonnet "create/enrich" planning call)
- `apply_wiki_ops` (append-only application)

**Stays exactly as built** — the debounce wrapper from v0.3.x is orthogonal and unchanged:
- `capture --in <secs>` (Stop-hook debounce; forks `capture --deferred`)
- the Haiku **triage** gate (its `NO` now narrowed to transient-or-redundant)
- the per-session **dedup marker** (by transcript exchange count)
- the per-session **flock**
- the SessionEnd `capture` path as the guarantee-of-last-resort

The agent loop slots in *after* triage passes, replacing the body that currently calls `distill_lessons`.

---

## Concurrency & locking

Captures run concurrently in the background (a debounce fork and the SessionEnd pass can fire close together). Two locks compose:

- **Per-session flock** (already in code): prevents two captures of *the same session* from running at once.
- **Per-project wiki write-lock** (new): the `wiki_*` mutating tools serialize on a project-level lock so two *different* sessions can't interleave edits to the same guide. On contention, the second writer re-reads before applying (optimistic check-on-write) so it never edits stale content.

---

## Global / user-perspective scope

The product-spec wiki is per-project. But some captured facts are **user-perspective** ("Pablo values depth over generic summaries"), global across all projects — distinct from **product-spec** facts ("avatars open a hovercard"). The existing `scope: global|project` split is the axis. Decision for this version: **global/user-perspective facts get the same citation-anchored treatment, in a global wiki** (`~/.proactive-context/global/`), via the same `wiki_*` tools pointed at the global root. The legacy `pending-lessons.md` append queue is retired in favor of this. *(Flagged as the one area to confirm before implementation — see Open Questions.)*

---

## Rejected approaches (with rationale)

Recording these once, compressed, so the detour isn't re-walked — and because "rejected approaches with rationale" is itself core spec content.

- **Structured evidence schema** (`quote` + `affirmed: bool` + `source_turn` + walk-back-to-proposal logic). Motivated by "quoting the user's *yes* is meaningless," it split evidence into a *content* axis and an *authorization* axis. **Rejected** because a single free-form `relevant_transcript` solves the same problem more simply: "include the relevant section" inherently captures the proposal *with* the affirmation, so "yes" is never quoted alone. The structured schema was solving a problem the lean field doesn't have. (That free-form string was itself later superseded by line ranges — see the next bullet — which remove even the need to verify a quote.)
- **Free-form `relevant_transcript` string + substring/fuzzy verification.** An intermediate design had the model *quote* the evidence and Rust verify the quote occurs in the transcript. **Rejected** in favor of line ranges: any matcher loose enough to tolerate the model reflowing a real quote also admits near-miss fabrications, and threshold tuning never ends. Ranges make integrity verbatim-by-construction and delete the matching question (see *The citation model*). Kept only as a documented fallback if ranges prove impractical.
- **Append-only enrichment** (never rewrite a guide). Rejected: product facts change; append-only accretes contradictions. Capture has the authoritative source and should restate.
- **Diff/string-match edits.** Rejected: reintroduces the offset-fragility and ambiguous-match problems we're escaping, and leaves Rust unable to know where to place the `[^id]`. Section-heading addressing replaces it.
- **Separate statement-ID registry.** Rejected as the same schema instinct over again: section headings are already stable anchors; a second ID space earns its complexity only if headings prove insufficient.
- **`Stop`-only capture without debounce** (the v0.2 position). Superseded by the debounce wrapper: `Stop` + 5-min debounce + dedup marker gives fresher capture without N-calls-per-session.

---

## Open questions / to verify before implementation

1. **`rig-core` fit (dependency-to-verify, not decided).** `rig-core` is already in `Cargo.toml` and would provide the tool-calling agent loop. Confirm its tool-use loop works against OpenRouter for Anthropic models. If it does not, we hand-roll multi-turn tool dispatch on blocking `reqwest` — a materially larger effort. Estimate hinges on this.
2. **Debounce foundation is empirically unverified.** Confirm with one live test: (a) the `setsid` child actually outlives the Stop-hook process exit, and (b) the real shape of `Stop`-hook stdin (especially whether `cwd` is present). The whole spec rides on this delivery path.
3. **Global wiki migration.** Confirm retiring `pending-lessons.md` for a citation-anchored global wiki, vs. keeping the lightweight append path for user-perspective facts.
4. **Evidence form — RESOLVED: line ranges, not a quoted string.** Evidence is line ranges into the line-numbered transcript (Rust slices verbatim), so there is no matching-tolerance question — extraction is verbatim by construction. (If evidence is ever kept as a string, the fallback is normalize+strict → reject-with-retry → marked fuzzy as last resort; see *The citation model*.) Remaining detail to confirm: how the transcript is line-numbered for the agent (turn-level vs raw-line) so ranges are stable and map back exactly when sliced.
5. **Triage sees the index?** To let triage's `NO` mean "already specified," it needs the wiki index in its prompt. Confirm feeding `wiki_list()` output into the triage call.

---

## Worked example (the format, applied to this very conversation)

A guide `capture-citation-model.md` might contain:

```markdown
## Evidence field

Capture records evidence as transcript line ranges; the model selects the
ranges that justify a decision and Rust slices the verbatim text — so the
model never quotes (and therefore cannot fabricate) the conversation. [^a3f9c5-7]
```

…with, in `_citations.log` (the verbatim text Rust sliced from the cited lines):

```
a3f9c5-7 | 2026-05-29T14:40Z | session:a3f9c5-… | Assistant: …have capture pick transcript line-ranges the way inject picks guide line-ranges — it deletes the verification problem rather than softening it. [...] Pablo: yes, good idea
```

The statement is the durable, restatable spec. The log entry — sliced by Rust straight from the cited transcript lines — is the immutable proof of *why*. That is the whole system in one example.
