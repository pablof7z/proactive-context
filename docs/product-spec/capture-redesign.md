# Capture Redesign — Product Spec

**Status:** Proposed (design-validated by empirical experiment; not yet implemented)
**Date:** 2026-05-30
**Scope:** The `capture` pipeline — how a finished conversation becomes durable project-wiki knowledge.

> **Why this document exists.** This session generated a large amount of design nuance — the *why*, the rejected alternatives, the empirical evidence, the philosophy — about how capture should work. That nuance is exactly the kind of thing the current capture pipeline *loses* (it fragments and accretes). Writing it down as one coherent authored spec is both the deliverable and a live demonstration of the problem we're fixing: **a coherent spec authored once beats nuance scattered across accreted, contradictory fragments.** Preserve the reasoning and the rejected paths, not just the conclusions — the "why we didn't do X" is the nuance that re-costs human time when lost.

---

## 1. Purpose & mission

`proactive-context` captures durable knowledge from coding sessions into a project wiki, then proactively injects relevant slices of it into future Claude Code sessions. **The wiki is the product.** Capture distills conversations into it; inject surfaces it.

Capture's mission is **not** to log what happened. It is to build a **desired-state product spec** that, crucially, also preserves the **archaeology of how that spec evolved** — because the deepest value of this tool is explaining *why the code looks the way it does*.

> A refactor that went A→B but didn't fully land leaves "weird" code that is *only* explicable if you know it used to be A. A spec that shows only the current state (B) throws away exactly the context that makes the weird code make sense. **Capturing the evolution of user decisions is a feature, not noise.**

The litmus test for whether capture is doing its job is operational — the **regeneration eval**: feed the accumulated wiki cold to a fresh model, have it rebuild the app, diff against the real app. The gaps are precisely the nuance capture dropped. (The wiki is currently far from one-shot-able — decisions/gotchas ~70% complete, definitional/structural knowledge ~10%.)

---

## 2. Foundational philosophy (invariants — do not violate)

These predate this redesign and constrain every option below.

1. **Positive, desired-state spec — not an event log.** Every statement describes how the product *should* work. *Wrong:* "avatar was broken." *Right:* "Tapping an avatar opens a hovercard with user details." (Refined by §6: desired-state **plus** curated provenance of how user decisions evolved.)
2. **Human time is irreplaceable; tokens are buyable.** A dropped nuance costs irreplaceable human time re-explaining it. Therefore capture is **recall-biased**: when uncertain, capture. (Refined by §5: recall-bias means don't drop the *fact* — it does **not** mean mint a new guide.)
3. **Distill, don't dump.** Raw transcripts are never embedded in the RAG index. The wiki is the distilled memory; the citation log is provenance that stays *out* of the embedding index.
4. **Citations are Rust-verified, not model-promised.** Every mutation carries `evidence` = transcript line ranges. Rust slices the verbatim text and confirms it actually occurs in the transcript, rejecting the call otherwise. This flips citation from "the model says this was said" to "Rust confirmed this was said." Hallucination of *evidence* is structurally impossible. (Note: this protects evidence fidelity, not the *accuracy* of synthesized prose — see §6/§7.)
5. **Integrity by construction.** Prefer designs that make violations *structurally impossible* over designs that merely *discourage* them with instructions. This is the through-line that makes the event-sourced destination attractive.
6. **Two-layer knowledge model.** *Layer 1* (decisions, gotchas, rationale) is mineable from conversations. *Layer 2* (entity/structural definitions: what the product is, schemas, contracts) **cannot** be mined — participants never state what they both already know — and must be **authored directly from code**. Capture addresses Layer 1; Layer 2 needs separate authored guides. (This spec itself is a Layer-2-style authored artifact.)

---

## 3. The problem: accretion

Capture today is one agentic tool-loop: a model gets the transcript + tools (`wiki_list`, `wiki_read`, `wiki_create`, `wiki_add_statement` [append-only], `wiki_revise_statement` [replace], `wiki_remove_statement`, `wiki_link`) and freely edits the wiki.

It **accretes**, in two distinct ways:

- **Cross-guide duplication** — it mints a fresh slug for a topic an existing guide already covers. The *only* guard is exact-slug collision, which never catches same-topic/different-slug duplicates.
- **Within-guide accretion** — it appends a new statement next to a contradictory old one instead of replacing it. `wiki_add_statement` only appends; `revise`/`remove` exist but the model is never told to prefer them.

### 3.1 Root cause — a decision-layer failure, and the central reframe

The machinery to update-in-place already exists and works. The failure is that **the model is never directed to use it**, and the only dedup guard is too weak. Combined with recall-bias ("when in doubt capture"), the path of least resistance is always append-or-create.

The deeper diagnosis — the central insight of this redesign:

> **Capture asks one generative pass to solve two retrieval-hard problems as side effects of writing prose:**
> 1. **ROUTING** — which guide/section does this fact belong to, or is it genuinely new?
> 2. **RECONCILIATION** — does this fact add to, replace, or retract what's already there?
>
> The cheap move (append/create) always wins. The fix is to **change the task shape** so routing and reconciliation stop being incidental generative judgment — hand the model the candidates (via retrieval) and constrain its choice. *"The catalog is the prompt."*

### 3.2 Why the easy fixes were rejected

- **Prompt-only fix (three "behave better" rules in the preamble): too hopeful.** It adds instructions to a recall-biased model that must still navigate four silent failure points per fact: notice an existing guide, choose to read it, judge add-vs-supersede, target the exact section. Each is a silent failure point.
- **Action-enum tool alone (`create/enrich/supersede/retract`): insufficient.** It makes intent *nameable*, not *correct* — the model can still pick `enrich` when it should `supersede` — and it does nothing for routing.

These aren't worthless (the action enum returns as part of RECONCILE), but neither is sufficient alone.

---

## 4. Architecture: the four-stage pipeline

Independently converged on by three reasoning agents. Capture stops being one free-edit pass and becomes a **reconciliation of a claim-set against the existing spec**.

```
EXTRACT → ROUTE → RECONCILE → HISTORY
```

1. **EXTRACT.** A model reads the transcript and emits **atomic, cited claims** — one fact each — with **authorship tagged mechanically** (see §5) and evidence ranges. No wiki access. Pure extraction.
   - *Quality dependency:* extraction is foundational — anything it misses is invisible to every later stage. (In testing, an un-extracted "jitter" proposal silently broke history downstream.)
2. **ROUTE.** Each claim is matched to **top-K candidate guides** — embedding RAG for recall, plus a **reasoning rerank** for semantically-distant but co-topical facts (e.g. "use optimistic locking" and "profile edits race" belong together but embed far apart). Output: a target slug, or "new topic."
   - **This is the highest-leverage stage** — see §8.1. If routing splits one topic across slugs, no downstream reconciliation can fix it.
3. **RECONCILE.** Claims are **grouped by target guide**, and **one pass per guide** sees the *full current guide* + *all claims routed to it* and chooses, per claim, `add / revise / remove / propose-new` (the action enum, now with a concrete target). Making the model *see the contradiction it would create* is the mechanism that actually prevents accretion.
   - **Per-guide, not per-claim. Sequential, not parallel.** (See §9 for why parallel-per-claim is a dead end.)
4. **HISTORY.** Supersedes and mind-changes are recorded in the claim/citation log; the guide projects the current live claim **plus** retained user-superseded provenance (§6).

### 4.1 The claim event

The unit of truth is a claim event:

```
claim_id · ts · session · author(user|agent) · slug · assertion · evidence(range+verbatim) · supersedes:[claim_id] · status
```

This is the existing `docs/wiki/_citations.log` **upgraded with schema** — not a new parallel log (§9).

---

## 5. Authority attribution: explicit vs implicit direction (the genuinely new primitive)

No prior alternative (embedding routing, capture-as-diff) had this axis. It reframes the wiki from *"facts the model believed"* to *"product direction, distinguishing what the principal stated from what the agent inferred."*

- **Mechanical, not classified.** Authorship is derived from **which transcript turn the evidence range falls in** — user turn → **explicit** direction; agent turn → **implicit** direction. Rust-checkable; no brittle LLM classifier.
- **Tag, don't drop (admission).** *Every* claim is admitted and captured. It is **tagged by authority**, not gated out:
  - **Explicit direction** (user-stated): load-bearing, permanent.
  - **Implicit direction** (agent-stated/inferred): captured but marked **provisional** — a real candidate product direction (often the actual implementation path), not yet blessed.
- **Lifecycle of an implicit claim:**
  - *default* — lives in the guide, clearly marked agent-inferred/provisional.
  - *user explicitly acknowledges it* → **promote** to explicit (permanent).
  - *user explicitly contradicts it* → **delete** the implicit claim (NO breadcrumb — it was never user intent, only an inference) and codify the explicit correction as permanent.
- **Worked example:** user "add oauth integration" (explicit) → agent "I'll add Google OAuth too" (implicit/provisional) → later user "only support github oauth" → the *google-oauth* implicit claim is **deleted**, "github-only oauth" codified explicit.

> **Design correction (2026-05-30).** This **replaces** the earlier "ratification gate" that *dropped* unratified agent claims at admission. Dropping was wrong on two counts: (1) it discarded the agent's inferred direction, which is usually the real implementation path and worth recording as provisional; (2) it destroyed coverage of *agentic* sessions (the archeologist test measured ~8 of 9 claims dropped from a delegated session — §8.3). Tagging-not-dropping captures everything *and* adds signal: a reader (or Claude) can see at a glance what is load-bearing user intent vs softer agent-taken direction.

---

## 6. Authority-asymmetric supersession retention (the archaeology rule)

When a claim is superseded/contradicted, retention depends on authority — the same explicit/implicit axis as §5:

- **Implicit (agent) claim contradicted by the user → DELETE** (no breadcrumb). It was an inferred direction, never blessed; once the user contradicts it there is nothing to memorialize. ("An agent inference being corrected is not relevant history.") Implicit claims are the *bulk* of all claims, so this keeps the corpus bounded.
- **Explicit (user) claim changed by the user → KEEP as a provenance breadcrumb**: *"Currently B (since session N). Was A until then — changed because X."* A user *mind-change* is genuine product history — the archaeology that explains why present-day code looks the way it does. User mind-changes are rare, so retained history grows slowly.

**Why:** this is the mission (§1). Current-tip-only projection discards the archaeology that explains present-day code. This **refines invariant 2.1**: the guide is desired-state **plus** curated provenance of *user-decision* evolution (not a raw event log — curated, terse, load-bearing).

**Bonus:** this gives pruning a *semantic* basis instead of a crude token-count heuristic — corrected agent inferences are disposable, user intent (and its evolution) is archival — which keeps projection input bounded (critical for the destination's cost; see §8).

**Open nuances:**
- *Granularity:* keep only evolutions with explanatory value; terse; not every trivial revision.
- *Agent facts vs agent proposals:* a contradicted agent *proposal/direction* is disposable, but a superseded agent *fact* (e.g. "the DB used sqlite-vec" before a swap) may be archaeology worth keeping. Default = delete contradicted implicit claims; revisit if a class of agent *facts* proves explanatory.

---

## 7. The destination architecture (event-sourced projections)

The integrity-by-construction endpoint:

> **Guides become read-only projections of the append-only claim log.** Nobody hand-edits guide prose — capture only *appends claim events* — so **accretion is structurally impossible.**

- **Capture emits events** (EXTRACT/ROUTE/RECONCILE decide NEW vs `supersedes`); `wiki_add_statement`/`wiki_revise_statement` as free-edit tools disappear.
- **A projection step re-renders touched guides** from their live claims.
- **Two projection options:**
  - *Deterministic render* — each claim a statement; cheap, stable, but list-like.
  - *LLM-assisted projection* — a model renders live claims into flowing prose, **constrained to cover exactly the live claim set** (add nothing, drop nothing), each sentence keyed to its `claim_id`. Readable; puts a (controlled) LLM back in the loop. (Analogous to — but distinct from — the inject **compile** stage, which synthesizes a per-prompt briefing with citations.)
- **Citation stability is preserved** because every statement is keyed to a stable `claim_id` that *owns* its evidence — re-projection reuses ids rather than scrambling sentence→citation positions. (The citation-carry-forward objection applies to *prose-keyed* regeneration; *claim-keyed* projection doesn't have it.)
- **Human edits are themselves logged** as top-authority user claims, so re-projection never destroys them — they become the head of a supersede chain.
- **Conflict detection:** two live contradictory claims with *no* supersede link between them = a **detectable anomaly**, flagged for the human — never silently merged. This is where the human stays in the loop.

### 7.1 Critical operational constraint: projection is a capture-time concern, never inject-time

There are **two distinct "read" moments**, and they must not be conflated:

| Moment | What happens | Latency-sensitive? |
|---|---|---|
| **Capture / compaction** (projection) | claim log → rendered guide `.md` on disk | No — runs on SessionEnd / on schedule |
| **Inject** (SELECT → COMPILE) | reads materialized `.md`, synthesizes a briefing | **Yes — fires on every prompt** |

**Guides are always materialized to disk; inject only ever reads materialized files.** Projection must **never** be deferred to inject-time — that would drop an expensive LLM render onto the pre-prompt hot path (the exact latency mistake corrected elsewhere). Whichever spin/architecture is chosen, **inject behaves identically** — the design only changes how the `.md` files are produced upstream.

---

## 8. Empirical findings (3 spins × 3 project types, on glm-5.1)

We prototyped three reconciliation strategies on synthetic scenarios (SaaS app, CLI tool, data pipeline), each engineered with a user mind-change, a corrected agent hallucination, a duplication trap, a within-guide contradiction, and a user-authority-vs-unratified-agent case. Scored against gold wikis. (Scorer caveat: a substring-based scorer over-counted violations — user claims that *name* the thing they replace tripped it — so raw metrics were corrected by reading actual outputs.)

### 8.1 The headline finding: ROUTING, not reconciliation, is the bottleneck

The thing we spent the most design energy on — write-time vs read-time reconciliation — **barely mattered**. Every spin reconciles *within a single guide*; **none handles a topic split across two slugs.** That cross-slug fragmentation broke all three (Spin 3 produced 11 guides for a 6-guide topic set; Spin 2's per-slug projector literally *cannot see* a supersede that landed in another slug). **The highest-leverage fix is ROUTE** — canonical, stable topic→slug, and/or cross-slug supersession — and it is upstream of the entire write-vs-read debate.

### 8.2 The spins

| Spin | Strategy | Result |
|---|---|---|
| **1 — smart-write / dumb-read** | Reconcile at write (action enum); deterministic projection of live claims | **Empirical winner.** 0 duplication, hallucinations excluded, keeps all facts, structured supersede history, cheap reads. Weaknesses: (a) **cascade gap** — superseding a rule doesn't retire *dependent* claims that only made sense under it; (b) plain readability (3.2–4.3). |
| **2 — dumb-write / smart-read** (purest event-sourcing) | Append every claim; LLM rebuilds guide from full history at read | Trivial writes, perfect history by construction — **but** every render is an LLM pass over the *full* history → **unbounded read cost** that grows for the life of the project; slowest; **did not finish**; structurally **most exposed** to routing fragmentation (per-slug read projector can't see cross-slug supersedes). Its elegance erodes toward Spin 1's bookkeeping the moment you care about cost. |
| **3 — coarse guide-regeneration** (no claim layer) | Regenerate whole guide each session from (prior guide + new facts) | Highest readability (4.5–4.9) — **but disqualified for a spec store:** it **silently drops facts** (SaaS coverage 4/6) and has **flat history with no supersede provenance** (can't support §6). Best prose, partly *because* it lost content. |

### 8.3 Recommended target: a Spin-1.5 hybrid

Spin 1's two weaknesses are Spin 2's two strengths, and §6 seals it:
- **Write-time supersede bookkeeping** (Spin 1) → bounded, cheap reads, structured history; GC of agent-superseded keeps it bounded forever.
- **plus a light LLM projection** that renders live claims into polished prose *and* weaves user-superseded claims in as "previously" archaeology → Spin 2's readability + §6's evolution context, **without** Spin 2's unbounded per-slug read cost or cross-slug blindness.

**This was gated on fixing routing first (§8.1) — now done (§8.4).**

### 8.4 Archeologist end-to-end validation on a real project (2026-05-30)

The v0.5 staged pipeline was run via `archeologist` over this project's own 47 sessions (44MB), into an isolated `--output-dir`, and compared against a hand-built **gold wiki** (27 guides assembled from a 297-claim digest of all sessions). Findings:

- **Routing-is-the-bottleneck: confirmed empirically.** The pipeline produced **33 guides from only ~half the sessions**, vs the gold's 27 from all — i.e. it *over-split* topics into near-duplicate slugs (`citation-id-format`≈`citation-markers`; `compile-model-as-librarian`≈`librarian-compile-model`).
- **ROUTE root cause (now known precisely, not guessed):** (1) `ROUTE_PREAMBLE` never defined guide *altitude*, so the model made one guide per *fact*; (2) new guides were created with **empty summaries**, so each session's index showed `slug | title | <empty>` and ROUTE was structurally blind to what existing guides covered. **Fixed** (commit `8d149c0`): define a guide as a subsystem-level chapter (~25-40/project) + thread real titles/summaries into the index. Result: **33→27 guides; citation-* 5→1; compile/librarian 4→1; empty summaries 33/33→0/27; zero grab-bags.**
- **Two infrastructure bugs found + fixed:** `--output-dir` did **not** redirect wiki writes (only markers) — a bulk run would have clobbered the real wiki (commit `d8e610f`); and the too-short gate counted strict user→assistant adjacency, silently dropping tool-heavy sessions (commit `f8db3b5`, now counts user turns).
- **Coverage is the standing limiter, and it motivated the §5 correction.** Even after the gate fix, only ~19/45 sessions contributed — because the *old* authority gate dropped unratified agent claims (one 113-msg session: 9 extracted, 1 admitted). This is precisely why §5 was changed to **tag-don't-drop**: implicit (agent) claims are now captured, not discarded, which should recover the agentic sessions. *Re-measuring coverage under the new model is the next experiment.*
- **Content quality where captured: good** (desired-state prose, correct citations, working `(Previously: …)` breadcrumbs). The weak layer was structure (routing), exactly as predicted.
- **Structural gaps no pipeline can mine** (confirmed vs gold): entity/definition guides, meta-rules (authority/supersession itself), positioning/philosophy, "rejected design" framing. These must be **authored directly** (Layer 2, §2.6).

### 8.5 Tag-don't-drop measured — and the fact-vs-proposal crux (2026-05-30, iter3)

Implemented §5 tag-don't-drop (commit `5d018fa`) and re-ran on this project. Result: deletion-on-contradiction fires cleanly (e2e: SMS + Kafka deleted); claim density recovered (+37% session-refs, 211 implicit claims admitted that the old gate dropped). **But it exposed a flaw in the mechanical model:**

- **Signal dilution (the headline):** **81% of admitted claims came out `implicit`** (211 implicit vs 50 explicit), because *in coding sessions the assistant narrates most settled architectural facts*. So `⟨provisional, agent-inferred⟩` landed on **208 statements across all 35 guides** — including core facts and product philosophy. The marker stopped discriminating.
- **Root cause:** "implicit = any agent turn" ≠ "implicit = unblessed *proposal*." The distinction actually wanted is **agent FACT** (*"the DB uses sqlite-vec"* — true, load-bearing) **vs agent PROPOSAL/INTENTION** (*"I'll add Google OAuth too"* — provisional). Mechanical turn-attribution can't separate them; both are assistant turns. This is the §6 *"agent facts vs agent proposals"* nuance, now confirmed load-bearing.
- **Promotion unreliable:** glm left an implicit claim provisional even after explicit user confirmation ("yes, do that").
- **Lifecycle under-exercised:** promote/delete only fires when an explicit and implicit claim co-route into the *same* reconcile batch — the §8.1 cross-slug limit again.

**Open fork (undecided):** make `implicit/provisional` mean *proposal*, not *agent-authored*. Most promising: have **EXTRACT classify claim TYPE (settled-fact vs proposal/intention)** — a narrower, phrasing-based classification ("I'll…/we should…/let me also…" vs declarative facts) that is far more tractable than the brittle *authority* classifier §3.2 rejected. Then only proposals carry the provisional marker; agent-narrated facts are plain content. The mechanical user/agent tag still drives the lifecycle internally; it just stops being the *rendering* signal.

---

## 9. Rejected alternatives (and why — preserve this nuance)

- **Prompt-only behavioral fix** — too hopeful; four silent failure points per fact (§3.2).
- **Action enum alone** — nameable ≠ correct; ignores routing (§3.2).
- **Parallel per-claim writes** — buys *zero* throughput (the wiki already serializes on a blocking `LOCK_EX`) and actively *re-creates* accretion, because parallel workers each reconcile against a **stale snapshot** and each picks the cheap append. Reconciliation requires seeing current full state; parallel workers don't. → Reconcile **per-guide, sequentially**.
- **A second append-only claim log** — would drift from the wiki with no reconciliation contract (becomes write-only, or re-injects accretion one layer down). The existing `_citations.log` is already the append-only store — **upgrade its schema** instead.
- **Full-wiki context broadcast every capture** ("hand the model every guide's title+summary+first-500-lines") — doesn't scale (the wiki is ~130 guides; "first 500 lines" ≈ the whole wiki), and duplicates what an upfront index already provides. → Use **top-K retrieval**, not broadcast.
- **Pure Spin 2 / pure Spin 3** — §8.2.

---

## 10. Open questions

1. **Routing (the priority).** Canonical topic→slug so an update lands where the original claim lives? Cross-slug supersession (a claim can retire a claim in a *different* slug)? This is the next experiment.
2. **The cascade gap.** When a rule is superseded, how do dependent claims that only existed because of it get retired? (Spin 1's main real failure.)
3. **Projection: deterministic vs LLM-assisted** — and if LLM, how to guarantee it covers exactly the live set (no silent drops, the Spin 3 failure mode).
4. **History granularity** — which user-superseded claims are worth keeping as archaeology vs which are clutter (§6).
5. **Compaction cadence** — eager (per capture) vs deferred (`wiki doctor`); and how aggressively to GC agent-superseded claims.
6. **Evaluation** — the substring scorer was unreliable; capture quality needs an LLM-judge / the regeneration eval, not string matching.

---

## 11. Roadmap

- **Increment 1 — EXTRACT/ROUTE/RECONCILE core. ✅ SHIPPED.** Staged pipeline replaces the free-edit loop; e2e tests (`e2e_revise_capture.sh`, `e2e_authority_gate.sh`) verify reconciliation-not-accretion. Routing over-split **fixed** (commit `8d149c0`, §8.4). Infra bugs fixed (`d8e610f`, `f8db3b5`).
- **Increment 2 — explicit/implicit direction tagging + authority-asymmetric retention. ◐ IN PROGRESS.** The §5 *tag-don't-drop* model (replacing the dropped "ratification gate") and the §6 archaeology rule. Prototype + coverage re-measurement is the active work.
- **Destination — event-sourced projections.** Upgrade `_citations.log` to the claim event-stream (§4.1); guides become projections (§7). The claim-with-author-and-evidence record is already ~80% of the schema this needs.
- **`pc wiki doctor` (deferred compaction)** — cheap insurance *regardless* of which prevention mechanism ships, because it is the **only** option that also **repairs duplicates already in the wiki**. Prunes contradicted implicit claims (per §6) and flags unlinked contradictions; periodic, claim-scoped — never continuous prose regeneration.

**Gating order:** ~~fix routing~~ ✅ → **explicit/implicit tagging + coverage re-measure** (active) → optionally evolve to the **destination** when multi-session scale forces it.

---

## 12. Model tiering note

The experiment used `glm-5.1:cloud` (a fast, non-reasoning instruct model) for all stages. In production, EXTRACT/ROUTE/triage favor fast/cheap tiers; RECONCILE and LLM-projection favor instruction-following quality. (A reasoning model on a fast gate is a known anti-pattern — reasoning models only answer correctly when reasoning is enabled, which is too slow for a latency-bounded stage.)
