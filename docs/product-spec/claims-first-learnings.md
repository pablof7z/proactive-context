# What the Claims-First Experiments Taught Us (Beyond the Verdicts)

**Status:** Living distillation. Runs 1–4 complete (2026-06-10), Run 5 (supersession rendering) in flight.
**Raw data:** `claims-first-validation-results.md` (all runs, frozen labels, judge prompts). Proposal: `claims-first-architecture.md`. Method: `claims-first-validation.md`.

This document exists because the experiment produced knowledge worth more than its headline verdicts. Whatever architecture we land on, these findings hold.

## 1. Findings about the system (architecture-independent)

**F1 — RECONCILE's supersession breadcrumbs empirically work.** 7/8 direction-change trajectories were recoverable from wiki briefings on a reversal-rich corpus. The standing critique ("history survives as at best one parenthetical, frequently nothing") was wrong and is retracted. Any future architecture must clear this 7/8 bar on direction evolution — it is now the measured floor, not an aspiration.

**F2 — The winner depends on corpus character.** Claims-first won an orchestration-heavy corpus (+19pt user-direction recall); the wiki won a design-heavy, reversal-rich corpus (trajectory 7/8 vs 3/8). Real projects are both. Conclusion: any single-store design must pass BOTH corpus types; passing one is not validation. The two preserved corpora (wallet `cfv2`, pc-own `cfv4`) are now the standing benchmark pair.

**F3 — The claim log's storage story is proven; its rendering story was the gap.** All 8 reversals were fully present in the claim store (both X and Y, dated, cited). The 3/8 trajectory failure was entirely the Phase-0 renderer surfacing only the latest claim. Storage-vs-rendering is the right decomposition for thinking about permanence: the wiki decides what to keep at write time (model judgment, lossy, worked better than expected); the log keeps everything and defers judgment to render time (lossless, only as good as the renderer).

**F4 — SELECT is the latency tail.** Skipping catalog navigation halved p95 latency in both runs (−44%, −51%) and cut input tokens ~2/3. Whatever wins, this finding applies to the current pipeline today: the guide-catalog SELECT stage is where the inject hot path's tail lives.

**F5 — Fact-confetti did not materialize.** 0/79 briefings judged incoherent across both corpora. The "atomic claims produce unreadable briefings" fear is, at this scale, unsupported.

**F6 — The whole pipeline ran on $0 of frontier spend.** Ollama Cloud (glm-5.1) + local fastembed handled extraction, mining, judging, and compilation for all four runs. Directly relevant to the broad-audience cost story.

## 2. Findings about technique (reusable machinery)

**T1 — Restatement mining works as a labeling method.** "Moments where the user re-told the agent something already established" are mineable from held-out transcripts, verifiable against history, and constitute naturally-labeled inject failures. The same mechanism, run live at capture time, is the production `inject.miss` detector — the eval and the product instrument are one design.

**T2 — Reversal mining works.** The miner independently rediscovered all 3 seeded direction reversals plus 5 unseeded ones, 8/8 verified present in-store. This is not just eval machinery: a reversal miner over claim history is a credible production mechanism for supersession/staleness detection.

**T3 — Temporal holdout replay is the right validation shape for this product.** Ground truth mined from what-actually-happened-next beats synthetic fixtures; it surfaced corpus-character effects no fixture set would have encoded. The eval harness (`pc eval`, on the experiment branch) should become the permanent regression suite for ANY pipeline change.

**T4 — Pre-registered criteria did their job.** Run 4 would have been easy to spin ("tied on recall, way cheaper!"). The written-down-first verdict forced the FAILS, which forced the diagnosis, which produced the decisive Run-5 experiment. Keep this discipline.

**T5 — Self-referential corpora need an injection guard.** Evaluating pc on its own history requires stripping pc's own injected briefings from user turns before mining, or labels become circular. Implemented as `strip_injected_context` + `is_pc_self_referential`; required for any future self-corpus run.

## 3. Findings about evaluation pitfalls (paid for in null runs)

**P1 — Silent skips produce confident nulls.** Runs 1–2 returned zero labels because `mine_labels` passed file content where a path was expected and every future session silently errored out. The null looked like "corpus has nothing to find." Lesson: eval plumbing must fail loudly; a zero from a miner is a claim that requires positive verification (e.g., seeded canaries — Run 4's seeded reversals were exactly this, and the miner had to find them before its N/A could be believed).

**P2 — Label-class scarcity is itself a finding.** The design-heavy corpus yielded only 3 explicit-direction labels out of 42 (the orchestration corpus: 16/37). The known 81%-implicit attribution skew shows up in eval labels too; explicit-direction conclusions need corpora or mining passes that target them deliberately.

**P3 — Clusters are not always versions.** Cosine clusters sometimes group co-occurring topical facts, not X→Y evolutions of one fact. Any logic that treats "older claim in cluster" as "superseded" will mislabel co-occurring facts; supersession needs a contradiction signal, not just age (Run 5 tests this).

## 4. Standing assets produced

- **Benchmark pair:** preserved stores + frozen labels for both corpora under `~/.proactive-context/experiments/` (`cfv2-…`, `cfv4-…`) — re-scorable without rebuild cost.
- **Harness:** claim-log tap (flagged), claims-inject path (flagged), `pc eval` runner, miners, judges — branch `worktree-agent-a757e309567dc3bd7` (results doc also copied to master).
- **The decisive open experiment:** Run 5 — deterministic cluster-timeline supersession rendering, success pre-registered as: trajectory ≥7/8, leaks ≤1/8, Probe-1 within 2pt, p95 still ≥30% better.
