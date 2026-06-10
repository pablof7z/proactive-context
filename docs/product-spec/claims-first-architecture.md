# Claims-First: Make the Claim Log the System of Record, the Wiki a Regenerable Projection

**Status:** Revision 3 — results from a second, adversarial corpus (2026-06-10). Verdict across corpora: **SPLIT — and the split is diagnostic.**
**Scope:** proactive-context core architecture (capture store, inject source, wiki's role).

## 0. Results: two corpora, two verdicts, one diagnosis

The proposal was tested twice by temporal holdout replay (spec: `docs/product-spec/claims-first-validation.md`): both stores built from the same replayed history, scored against labels mined from held-out future sessions, with pre-registered pass/fail criteria written before each run.

**Run 3 — wallet/orchestration corpus (25 sessions, 37 labels): PROMISING.**
Restatement recall on user-direction facts: claims 81.2% vs wiki 62.5% (+19pt on the kill criterion). p50 latency −44%, input tokens −61%, 0/37 incoherent. Direction-change probe not evaluable (no reversals in window).

**Run 4 — pc's own design-heavy corpus (30 sessions, 42 labels + 8 verified direction-reversals): FAILS.**
Restatement recall tied overall (69.0% both). But on **direction-change fidelity** — the first corpus where it was measurable — the wiki won decisively: trajectory X→Y recoverable in 7/8 briefings vs claims' 3/8; stale-direction leaks 1/8 vs 2/8. Concrete failure: on the embeddings-provider reversal, the wiki briefing carried *"this replaced OpenAI's 1536-dim model"* while the claims briefing stated the current truth and **dropped the history**. Operational metrics still favored claims (p95 −51%, tokens-in −67%), but the pre-registered verdict on this corpus is FAILS.

**The diagnosis is structural and specific.** The Phase-0 claims-inject path retrieves the *latest* claim per cluster and renders it; it never implemented the cluster-aware supersession rendering this proposal specified in §5 ("supersession within a cluster must render as *current Y (was X)*"). Meanwhile the wiki's RECONCILE breadcrumbs — which this proposal's §4 dismissed as fragile — empirically worked: 7/8 trajectory preservation. **That criticism is hereby retracted as too pessimistic.** The claim log still *holds* the full trajectory (both X and Y verified present in-store for all 8 reversals); the Phase-0 renderer simply doesn't surface it.

**Revised read:** claims-first wins where the load is breadth of direction recall (orchestration-style work) and loses where the load is direction *evolution* (design-heavy work) — exactly until supersession rendering exists. The next experiment is sharp and cheap: implement "current Y (was X)" rendering from cluster history in the claims compile path, then re-run the direction-change probe against the preserved Run-4 stores (no rebuild needed). If claims-first then matches the wiki's 7/8 while keeping its recall and cost wins, the architecture case closes. If it can't, the wiki's write-time reconciliation is doing something projection cannot cheaply replicate, and this proposal should be narrowed or withdrawn.

## 1. The goals this must serve

1. **Inject quality is the north star.** Failure = missing or incorrect injection/capture. Missing human direction is an absolute sin; missing inferable observations is less bad but still bad.
2. **Permanence of human direction.** Every nuance accrued from user direction over time is permanently stored — *including changes of direction*. The barometer: after thousands of sessions, one-shot the entire project from the distilled, current-truth product spec.

## 2. The claim

> **The append-only claim log becomes the system of record. The wiki becomes a derived, disposable, regenerable projection of it. Inject sources from claims (and/or fresh projections).**

## 3. The evidence: the store representation drives the documented pain

Every major documented battle — routing as the empirical bottleneck, the 172-vs-27 altitude failure, RECONCILE fighting accretion, `doctor` repairing routing mistakes — is a fight to maintain one invariant: *"each fact lives in exactly the right prose file."* That invariant exists because inject retrieves at guide granularity. The pipeline produces the ideal artifact (atomic, cited, authority-tagged claims), dissolves it into prose, then pays COMPILE to re-extract facts from that prose. ROUTE — the hardest stage — is an artifact of the chosen store.

## 4. Permanence (amended by Run 4)

The claim log makes permanence **structural**: every direction state survives with timestamp, authority, citation; a claim cluster across sessions *is the trajectory of the user's thinking*. Run 4 confirmed the storage half (all 8 reversals fully present in the claim store) while refuting this section's original swipe at prose breadcrumbs (they preserved trajectory 7/8). The honest restatement: **the wiki forgets less than argued, but what it keeps is decided by a model at write time; the claim log keeps everything and the question is purely whether the renderer surfaces it.** The one-shot regeneration test remains a projection operation, best run over complete history with hindsight — and the archeologist already demonstrated replay beats live accretion.

## 5. The resulting shape

- **Capture:** TRIAGE (must never veto sessions containing user direction) → EXTRACT → mechanical authority tagging → append to claim store (embedded, cosine-clustered, no LLM). ROUTE and RECONCILE leave the write path.
- **Inject:** retrieve claim clusters → rank deterministically (explicit user direction first, recency, similarity) → COMPILE renders pre-ranked cited claims, **including cluster supersession as "current Y (was X)" — now demonstrated by Run 4 to be load-bearing, not optional.**
- **Projection (offline):** compile clusters into the current-truth spec; errors cosmetic, fixed by re-running.
- **Fallout:** claims need not live in the user's repo; opens hybrid push/pull inject; ledger, doctor-merge, topic-routing, staleness machinery shrink.

## 6. Open questions, re-ranked by the data

1. **Supersession rendering** (was open question #4-ish, now #1): implement and re-test against the preserved Run-4 stores. This is the decisive experiment.
2. **Implicit-recall variance:** B regressed on implicit facts in Run 3 (52% vs 62%) yet edged A in Run 4 (72% vs 69%) — corpus-dependent; needs a wider-retrieval ablation.
3. **Probe 1 explicit-label scarcity:** Run 4 had only 3 explicit labels (n too small to read). Label mining needs an explicit-direction-targeted pass.
4. **Coherence at scale:** 0/79 incoherent across both runs is encouraging; larger claim volumes untested.

## 7. One-line summary

The claim log provably keeps everything and injects cheaper; the wiki provably tells the story of how direction changed; the next experiment — supersession-aware rendering over the preserved stores — decides whether the log can also tell the story, which is the whole question.
