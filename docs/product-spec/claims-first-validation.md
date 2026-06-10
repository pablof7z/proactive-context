# Claims-First Validation: Temporal Holdout Replay

**Status:** Approved for build-out (experiment, feature-flagged — must not disturb the existing pipeline).
**Companion to:** the `claims-first-architecture` proposal (published on tenex-edge fabric).

## 1. Question under test

Does sourcing inject from an **append-only claim store** (Store B) produce better briefings than the current **prose wiki guides** (Store A) — measured against ground truth mined from real session history?

"Better" per the project's ratified failure hierarchy:
- Missing human direction in a briefing = absolute sin.
- Missing inferable observation = bad.
- Asserting superseded/stale facts as current = incorrect injection, also a sin.

## 2. Method: temporal holdout replay

1. Pick a real project corpus with long transcript history (see §6 for corpus selection).
2. Split its sessions **chronologically**: first ~80% = HISTORY, last ~20% = FUTURE (held out).
3. Build both stores from HISTORY only, via the existing archeologist replay machinery:
   - **Store A (incumbent):** the current pipeline as-is → wiki guides in a temp wiki dir.
   - **Store B (claims-first):** the same EXTRACT + mechanical authority-tagging output, persisted as an append-only claim log instead of being routed/reconciled into prose.
4. Score both stores against ground truth mined from FUTURE (probes in §4).
5. The FUTURE sessions are never used to build either store. They exist only to generate labels.

## 3. What must be built (Phase 0 — the only new engineering)

### 3.1 Claim store persistence
- After EXTRACT + authority tagging (capture.rs, `run_staged_capture` stages 1–2), persist every admitted claim to an append-only JSONL claim log, one record per claim:
  ```json
  {"id": "...", "ts": "<transcript-date>", "session": "...", "assertion": "...",
   "authority": "explicit|implicit", "evidence_text": "<verbatim slice>",
   "evidence": [{"start": N, "end": M}]}
  ```
- Location: `~/.proactive-context/projects/<key>/claims.jsonl` (or alongside, agent's choice — NOT in the user repo).
- Feature flag (config field or env var, e.g. `PC_CLAIMS_LOG=1`). Default OFF. When ON, the rest of the pipeline (ROUTE/RECONCILE) still runs normally — the claim log is a tap, not a fork, so one archeologist replay builds BOTH stores in a single pass (one EXTRACT spend, two stores).
- Embed claims (existing fastembed local embedder) into a sqlite-vec table (`claims.db` or a new table in the existing schema). Cosine-cluster near-duplicates (mechanical, threshold tunable, start at the route_recall tau machinery as reference). A cluster = one fact's history over time.

### 3.2 Claims-inject variant
- A flagged inject path (`PC_INJECT_SOURCE=claims` or similar): given the (enriched) query —
  1. Vector-retrieve top claim clusters.
  2. Rank deterministically: authority first (explicit user direction outranks implicit), then recency within a cluster (latest claim in a cluster = current truth; earlier claims = visible history), then similarity.
  3. COMPILE (existing compile model/prompt, lightly adapted): format the pre-ranked cited claims into the briefing. Supersession within a cluster must render as "current: Y (was: X)" — never assert a superseded claim as current.
- Keep the existing SELECT stage OUT of this path (no catalog navigation needed); measure what pure retrieval+rank+format does.

### 3.3 Eval runner
- A script or `pc eval` subcommand that, given a corpus and split point:
  - builds both stores from HISTORY (drives archeologist with the claim-log flag on);
  - mines labels from FUTURE (§4);
  - replays probe prompts through both inject paths;
  - emits a results report (markdown + raw JSONL of every probe result).
- Determinism note: record every LLM call's model + prompt hash in the results so runs are comparable.

## 4. Probes (v1 scope)

### Probe 1 — Restatement recall (MUST HAVE)
- **Label mining:** scan FUTURE sessions for user turns that restate, re-explain, or re-correct something already established in HISTORY. One offline LLM pass (cheap model acceptable) proposes candidates: `{future_session, future_prompt_before_restatement, restated_fact, history_evidence}`. Each candidate must be verified to actually exist in HISTORY (grep/embedding match against HISTORY transcripts) — discard unverifiable labels. Freeze the label set before scoring.
- **Scoring:** for each label, run the *real* preceding FUTURE prompt through Store A inject and Store B inject. Did the briefing contain the restated fact (LLM judge: contained / partially / absent)? Report recall overall and split by authority (user-direction labels reported separately — these are the sin meter).

### Probe 2 — Direction-change fidelity (SHOULD HAVE)
- **Label mining:** find reversals in HISTORY (user established X, later overrode with Y). Same mining+verification discipline.
- **Scoring:** probe both stores with on-topic queries; score (a) briefing asserts Y as current, (b) briefing never asserts X as current (leak = incorrect injection), (c) X→Y trajectory recoverable.

### Probe 3 — Operational metrics (MUST HAVE, free)
- Per-inject wall-clock p50/p95, token counts in/out, $ estimate, briefing size, for both paths over all probe runs.

### Deferred to v2 (do NOT build now)
- Stale-citation rate vs contemporaneous git commits.
- Scoped regeneration probe.
- Blind multi-judge panel; v1 uses a single strong-model judge with the verdict prompt logged verbatim in results.

## 5. Pre-registered read (write down before running)

Store B is *promising* if, on the frozen label set:
- Probe 1 recall on **user-direction labels** ≥ Store A (parity or better is the bar; a loss here kills the proposal regardless of other wins);
- Probe 2: strictly fewer stale-assertion leaks than Store A;
- Probe 3: meaningfully cheaper/faster (target ≥30% latency reduction given SELECT is skipped);
- Briefing coherence not catastrophically worse (judge flags "incoherent / fact-confetti" on < 20% of B briefings).

Anything else = report honestly, no spin. A mixed result is a finding, not a failure.

## 6. Corpus and cost control

- **Smoke test first:** run the whole harness end-to-end on `~/src/pc-wikitest` (the "Lumen" controlled project) to debug the machinery cheaply before touching real corpora.
- **Real corpus:** ONE project for v1. Prefer the richest available history under `~/.claude/projects/` (the nostr/Cashu wallet project is the known stress case; podcast-player is the fallback). **Cap HISTORY replay at ~30 sessions** (chronologically contiguous, ending at the split point) to bound OpenRouter spend; note the cap in results.
- Reuse the user's existing `~/.proactive-context/config.json` models/key. Log total spend.
- All store outputs go to temp/experiment directories — never write into the corpus project's repo, and never touch the user's live `~/.proactive-context/projects/<key>` state for that corpus (use a distinct experiment key or `PC_HOME` override if one exists; add one if trivial).

## 7. Deliverables

1. Code on a branch (small, frequent commits): claim-log tap, claims-inject path, eval runner. Feature-flagged, zero behavior change when flags are off; `cargo test` stays green.
2. `docs/product-spec/claims-first-validation-results.md`: the frozen labels, every probe result, the operational table, the pre-registered read applied verbatim, and a short honest narrative of what was observed — including harness bugs and label-quality problems.
3. Raw artifacts (results JSONL, built stores) parked under the experiment dir, path noted in the report.
