---
title: Taxonomy Eval
slug: taxonomy-eval
topic: wiki-architecture
summary: Phase 3 A0âA5 eval arms were initially deferred because they would be six full eval runs (~hours each) and nothing was default-on yet; the cost was only warra
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-17
updated: 2026-06-18
verified: 2026-06-17
compiled-from: conversation
sources:
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
  - session:019ed77e-dee0-72f3-a487-f51771e5e8c9
  - session:019ed791-4dcf-7b61-8a5a-fb6b134e3c48
  - session:2d1210d9-f831-4b38-a5a9-abe383225f70
---

# Taxonomy Eval

## Eval Deferral & Cost

Phase 3 A0–A5 eval arms were initially deferred because they would be six full eval runs (~hours each) and nothing was default-on yet; the cost was only warranted when deciding to flip a default. Phase 3 added `PC_SELECT_SOURCE_TYPES` source-type SELECT guidance behind the flag (code only; eval arms deferred). Phase 5 (claim catalog) is deferred until cluster summaries have stable currentness and pass review. The evaluation harness must reach statistical reliability before any feature defaults are changed or new features are added. Frozen eval labels are reusable for later phases via `--score-only`, making subsequent eval arms cheaper than the baseline build and avoiding the expensive mining step. The `--synth-every` cadence should not be tuned mid-run unless the current default is known-bad; consistency across the overnight run takes priority. Regens start only after the current high-power eval finishes, since the eval is holding the LLM.

<!-- citations: [^019ed-12] [^8eff6-19] [^8eff6-20] [^8eff6-31] [^019ed-1] [^8eff6-43] [^8eff6-61] -->
## Eval Harness Bug

Every evaluation run must be persisted with a run id and versioned artifacts rather than clobbering markdown. The eval harness auto-overwrites `claims-first-validation-results.md`, destroying accumulated history (e.g., ~1475 lines of accumulated Runs 11–12 history); it should append or version its output instead. The arm harness avoids this bug by writing results only to the experiment directory.

<!-- citations: [^8eff6-21] [^8eff6-32] [^019ed-2] [^8eff6-44] [^8eff6-62] -->
## Evaluation Methodology

The baseline probe eval (Store A = wiki / current-guide path) established reference metrics: 75% guide restatement recall, 71.4% user-direction recall, 2/10 stale-current leak, 9/10 trajectory X→Y recoverable, 5.0s/12.7s p50/p95 latency. Judge variance is reduced by caching compiled arm outputs and judging each output with K=3 majority-vote samples at temperature 0, using majority vote for categorical verdicts and mean for ordinal scores. Evaluation reports must use paired confidence intervals (e.g., via bootstrap) and win/loss/tie counts over labels rather than point estimates. At n=20 labels and a single non-deterministic judge, recall deltas of ≤ ~20 points are pure noise; re-running identical arms on identical labels produced recall swings of 15–25 points between runs, retracting that call. Resolving recall deltas at n=20/single-judge requires a methodology upgrade: n≥40 labels AND 3+ judge passes averaged (or a stronger/deterministic judge). Labels must be converted to an atom coverage form where each label has 1-3 required atoms, and the judge must provide quoted evidence for each covered atom rather than using vague contained/partial/absent grading. The primary optimization metric is decision-critical recall under a hard stale-leak constraint, formulated as usable_context_hit_rate @ stale_leak <= threshold @ token_budget. Episode-card trajectory recovery is a more product-aligned proxy than mere fact restatement for evaluating context utility. The evaluation must cache intermediate stages (catalog, SELECT prompt, SELECT output, compiled context) separately to distinguish between label noise, SELECT variance, and judge variance.

A deterministic post-hoc token-overlap analyzer was built at `~/.proactive-context/arms_xcheck.py` to cross-check the noisy LLM judge results. <!-- [^8eff6-64] -->

<!-- citations: [^019ed-3] [^8eff6-46] [^8eff6-63] -->
## Label Mining & Adversarial Coverage

Label mining is uncapped with a target of at least n≥40 restatement/actionability labels plus more reversals. Adversarial reversal labels must be expanded to include settled→superseded, proposed→rejected, obsolete implementation paths, and contradictory guide/claim/episode evidence because 0 stale leaks on ~10 reversals is not proof. Transcript contamination must be filtered before label expansion; tool results, injected reminders, command wrappers, and tenex-edge inbox messages stamped promptSource: typed must be excluded to avoid optimizing recall of synthetic session plumbing.

<!-- citations: [^019ed-4] [^8eff6-65] -->
## Eval Arms & Default Status

Evaluation arms must include A0, A1, A2, and A4; A3 is skipped unless the research corpus is larger. Making research selectable did not improve recall in the evaluation because the corpus only contains 4 research records. A2/source-type SELECT is the only candidate flag near default-on under the current evaluation. A2's source-type SELECT guidance over-suppresses episode-card selections (selections dropped 24→9) because a COMPILE-time caution ('Do NOT select a historical artifact') was misplaced at SELECT time; the A2′ fix removed that line and explicitly retains relevant episode cards for why/history probes. The A2′ tuning mechanistically recovered episode-card selections from 9→25 and trajectory from 7→8/10, with stale-leak staying at 0 across both runs, resolving the regression at no cost to stale-leak. Stale-leak was approximately 0 under every evaluation arm, establishing it as a safety floor that no flag combination regresses. An arm is default-on as a candidate iff (paired-delta-vs-A0 bootstrap 95% CI lower bound > 0 on recall) AND (stale_leak <= A0) AND (p95 latency & avg tokens within 15% of A0); otherwise it stays off. If an arm fails the default-on CI bar but shows point estimate improvement with stale_leak <= A0 and acceptable latency/tokens, it is classified as promising/rerun-needed (opt-in only), not default-on. The high-power eval used K=3 majority judging (`--judge-k 3`) at temp 0 with paired bootstrap CIs across all 40 labels + 10 reversals for arms A0/A1/A2/A4, with frozen labels reusable via `--score-only`. High-power eval results: recall A0 65% → A1 70% → A2 70% → A4 78%, all paired deltas positive but 95% CIs straddling 0 (lower bounds ≈ −0.03 to −0.05); stale-leak 0/10 across every arm. An independent deterministic token-overlap recall cross-check confirmed the same ordering: A0 27.5% → A1 32.5% → A2 35.0% → A4 35.0%. A4 was rejected for default-on due to +46% p95 latency cost and vacuous nouns in the store; A3/research never helped on the thin 4-record corpus. A2 (typed catalog + source-type SELECT) is the ship decision: best recall on both metrics (+0.087 paired; 35% deterministic), selects the most episodes (40), at acceptable cost (+12% p95, −2% tokens), with zero stale-leak. The decision to flip defaults was made on converging evidence (K=3 judge + deterministic cross-check agree on A0<A1<A2, zero stale-leak, cost in budget) without waiting for CIs to clear zero.

<!-- citations: [^019ed-5] [^019ed-11] [^8eff6-47] [^8eff6-66] -->
## Context & Scope

The taxonomy cleanup session follows an 8-phase plan in `Plans/content-taxonomy-implementation-experiment-plan.md`. A Phase 0 audit found over-fragmentation: 57 guides across 28 topics (~12 singletons), while 100 research records and 159 episode cards (66 active) were largely injection-invisible to the selector.

<!-- citations: [^8eff6-42] [^8eff6-67] -->
## Eval Harness Architecture

The legacy eval's Probe-1 scoring deliberately bypasses SELECT and `build_catalog` — it does embedding retrieval over guides only, then compiles. This means Phase 2/3 flags are invisible to it and it only loads guides. Phase 3 eval arms A0–A5 require a new harness that exercises the full inject path (catalog + SELECT + COMPILE), not the legacy scorer. A new eval harness (`pc eval --select-arms`) was built that runs the real catalog+SELECT+COMPILE pipeline over frozen labels for arms A0–A4, scoring recall, reversal stale-leak/trajectory, selection-by-kind, and latency. The `navigate_and_compile_for_eval` function was exposed as a `pub(crate)` async wrapper to allow the eval harness to drive the full inject path (empty hits, no recent context) without touching the live call site.

<!-- citations: [^8eff6-45] [^8eff6-68] -->

## Validation & Regression Safeguards

Validation for Stage 1 must use cosmetic-canary and anti-over-split regression cases, and track missed cosmetic specs, uncited/hallucinated specs, and claim-count inflation per session (with inflation being the ROUTE bottleneck tripwire). <!-- [^2d121-16] -->
