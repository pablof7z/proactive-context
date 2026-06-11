# What the Claims-First Experiments Taught Us (Beyond the Verdicts)

**Status:** Living distillation. Runs 1–5 complete (2026-06-10/11).
**Raw data:** `claims-first-validation-results.md` (all runs, frozen labels, judge prompts). Proposal: `claims-first-architecture.md`. Method: `claims-first-validation.md`.

This document exists because the experiment produced knowledge worth more than its headline verdicts. Whatever architecture we land on, these findings hold.

## 1. Findings about the system (architecture-independent)

**F1 — RECONCILE's supersession breadcrumbs empirically work.** 7/8 direction-change trajectories were recoverable from wiki briefings on a reversal-rich corpus. The standing critique ("history survives as at best one parenthetical, frequently nothing") was wrong and is retracted. Any future architecture must clear this 7/8 bar on direction evolution — it is now the measured floor, not an aspiration.

**F2 — The winner depends on corpus character.** Claims-first won an orchestration-heavy corpus (+19pt user-direction recall); the wiki won a design-heavy, reversal-rich corpus (trajectory 7/8 vs 3/8). Real projects are both. Conclusion: any single-store design must pass BOTH corpus types; passing one is not validation. The two preserved corpora (wallet `cfv2`, pc-own `cfv4`) are now the standing benchmark pair.

**F3 — The claim log's storage story is proven; its rendering story was the gap.** All 8 reversals were fully present in the claim store (both X and Y, dated, cited). The 3/8 trajectory failure was entirely the Phase-0 renderer surfacing only the latest claim. Storage-vs-rendering is the right decomposition for thinking about permanence: the wiki decides what to keep at write time (model judgment, lossy, worked better than expected); the log keeps everything and defers judgment to render time (lossless, only as good as the renderer).

**F4 — SELECT is the latency tail.** Skipping catalog navigation halved p95 latency in both runs (−44%, −51%) and cut input tokens ~2/3. Whatever wins, this finding applies to the current pipeline today: the guide-catalog SELECT stage is where the inject hot path's tail lives.

**F5 — Fact-confetti did not materialize.** 0/79 briefings judged incoherent across both corpora. The "atomic claims produce unreadable briefings" fear is, at this scale, unsupported.

**F6 — The whole pipeline ran on $0 of frontier spend.** Ollama Cloud (glm-5.1) + local fastembed handled extraction, mining, judging, and compilation for all five runs. Directly relevant to the broad-audience cost story.

**F8 (Run 6) — The supersession bottleneck bottomed out at EXTRACT.** Capture-time supersedes edges (dual-channel candidate retrieval + LLM contradiction judgment) WORK as machinery (167 edges; the one cleanly-extracted reversal linked and rendered correctly) — but 4/8 canonical reversals were never extracted as contradictions at all: EXTRACT phrases "the default flipped from X to Y" as co-existing capabilities ("can use local provider"), which contradicts nothing. The arc walked rendering (Run 5) → retrieval (Run 6 edges) → extraction; claims-first supersession requires replacement-aware EXTRACT, a capture redesign. Cost finding: edge detection = 259 extra LLM calls, 26→82 min store build (3.1×). Topline confound: at n=8 with a fresh judge and corpus drift, Store A also swung 7/8→2/8 — small-n cross-run toplines are variance; the per-reversal diagnostic is the trustworthy signal (P4 generalizes).

**F7 (Run 5) — Supersession is a relation-detection problem, not a rendering problem.** With deterministic CURRENT/SUPERSEDED/RELATED timeline rendering built (contradiction-gated, Rust-side), Store B improved (leaks 2/8→1/8, current-assertion 6/8→7/8, no recall regression, p95 still 32% better) but trajectory only reached 4/8 vs the wiki's ~7/8. Root cause, measured: **7 of 8 reversals have X and Y in DIFFERENT cosine clusters** — a reversal's replacement is phrased unlike what it replaces ("OpenRouter API embeddings" vs "local MiniLM embeddings"), so similarity clustering splits the very pairs supersession needs joined. This is WHY the wiki wins on trajectory: routing forces both claims into the same guide and RECONCILE — an LLM — sees the contradiction side by side. The LLM in the write path is doing real semantic work (contradiction linking) that mechanical clustering cannot replicate. Next experiment if pursued: explicit `supersedes` edges recorded at capture time (compare each new claim against top-K similar+related existing claims with a small LLM step) — i.e., a slimmed RECONCILE over the log; the now-working renderer then has the edges it needs. Run 5 verdict per its pre-registered frame: PARTIAL (3/4 criteria pass; trajectory decisive miss).

## 2. Findings about technique (reusable machinery)

**T1 — Restatement mining works as a labeling method.** "Moments where the user re-told the agent something already established" are mineable from held-out transcripts, verifiable against history, and constitute naturally-labeled inject failures. The same mechanism, run live at capture time, is the production `inject.miss` detector — the eval and the product instrument are one design.

**T2 — Reversal mining works.** The miner independently rediscovered all 3 seeded direction reversals plus 5 unseeded ones, 8/8 verified present in-store. This is not just eval machinery: a reversal miner over claim history is a credible production mechanism for supersession/staleness detection.

**T3 — Temporal holdout replay is the right validation shape for this product.** Ground truth mined from what-actually-happened-next beats synthetic fixtures; it surfaced corpus-character effects no fixture set would have encoded. The eval harness (`pc eval`, on the experiment branch) should become the permanent regression suite for ANY pipeline change.

**T4 — Pre-registered criteria did their job.** Run 4 would have been easy to spin ("tied on recall, way cheaper!"). The written-down-first verdict forced the FAILS, which forced the diagnosis, which produced the decisive Run-5 experiment. Keep this discipline.

**T5 — Self-referential corpora need an injection guard.** Evaluating pc on its own history requires stripping pc's own injected briefings from user turns before mining, or labels become circular. Implemented as `strip_injected_context` + `is_pc_self_referential`; required for any future self-corpus run.

## 3. Findings about evaluation pitfalls (paid for in null runs)

**P1 — Silent skips produce confident nulls.** Runs 1–2 returned zero labels because `mine_labels` passed file content where a path was expected and every future session silently errored out. The null looked like "corpus has nothing to find." Lesson: eval plumbing must fail loudly; a zero from a miner is a claim that requires positive verification (e.g., seeded canaries — Run 4's seeded reversals were exactly this, and the miner had to find them before its N/A could be believed).

**P2 — Label-class scarcity is itself a finding.** The design-heavy corpus yielded only 3 explicit-direction labels out of 42 (the orchestration corpus: 16/37). The known 81%-implicit attribution skew shows up in eval labels too; explicit-direction conclusions need corpora or mining passes that target them deliberately.

**P3 — Clusters are not always versions, and versions are often not clustered.** Cosine clusters sometimes group co-occurring topical facts (age ≠ supersession — Run 5's contradiction gate handles this), and — the sharper Run-5 finding — true X→Y versions usually DON'T cluster, because replacements are phrased unlike what they replace. Similarity is the wrong signal for supersession in both directions.

**P4 — Single-judge non-determinism is material.** On the same frozen labels, Store A's Probe-1 score moved 69.0%→78.6% between Runs 4 and 5 with zero store changes. Cross-run comparisons of a single judge carry ~±5-10pt noise; within-run A-vs-B comparisons are safer, and the wiki's "7/8 floor" itself has error bars. Multi-judge or pinned-seed judging needed before small deltas are trusted.

## 4. Standing assets produced

- **Benchmark pair:** preserved stores + frozen labels for both corpora under `~/.proactive-context/experiments/` (`cfv2-…`, `cfv4-…`) — re-scorable without rebuild cost.
- **Harness:** claim-log tap (flagged), claims-inject path (flagged), `pc eval` runner, miners, judges — branch `worktree-agent-a757e309567dc3bd7` (results doc also copied to master).
- **Run 5 outcome:** PARTIAL — rendering built and working (leaks ≤1/8, no recall regression, p95 −32%), trajectory 4/8 blocked by cross-cluster reversals (F7). `PC_CLAIMS_RENDER=legacy` toggle reproduces Run 4.
- **The decisive open experiment (next):** capture-time `supersedes` edges — a small LLM contradiction-link step per new claim against top-K similar existing claims (a slimmed RECONCILE over the log). Requires a store rebuild; success bar inherits Run 5's frame with trajectory ≥7/8.
