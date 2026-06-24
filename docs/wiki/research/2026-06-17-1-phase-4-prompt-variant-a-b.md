---
type: research-record
date: 2026-06-17
session: 0323ebcf-373e-4e5d-b1c6-8dac16f3055d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/0323ebcf-373e-4e5d-b1c6-8dac16f3055d.jsonl
source_lines: 7611-7649
agent_attribution: a5389aaa300fea689
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Phase 4 prompt-variant A/B eval: 7 arms adjudicated against pre-registered bars — verdict: no wins, C2 KILLED for fact-coverage regression, inject arms unadjudicable due to harness wiring gap"
captured_at: 2026-06-17T21:37:05Z
---

Phase 4 prompt-variant A/B eval: 7 arms adjudicated against pre-registered bars — verdict: no wins, C2 KILLED for fact-coverage regression, inject arms unadjudicable due to harness wiring gap

---

User: [Agent task result: Agent "Phase 4: run prompt-variant evals" completed]
All seven arms run to completion on frozen cfv6 with glm-5.1 cloud (think-ON, $0), results committed as `b260846`.

## Per-arm adjudication (verbatim PASS/FAIL vs pre-registered bars)

B0 is the load-bearing column (what each toggle can actually move). Baseline I0 = C0 (librarian compile over frozen base store-b).

| Arm | B0 noun-grounding | B0 predict (any-signal) | B0 restatement P1 | store-b claims | Verdict |
|---|---|---|---|---|---|
| **I0=C0 baseline** | 21.4% | 30% | 85.7% | 260 | — |
| I1 verdict | 35.7% | 10% | 76.2% | 260 (frozen) | **NULL/unadjudicable** |
| I2 divergence | 14.3% | 25% | 81.0% | 260 (frozen) | **NULL/unadjudicable** |
| S1 select | 14.3% | 20% | 85.7% | 260 (frozen) | **NULL/unadjudicable** |
| C1 typed | 14.3% | 25% | 69.0% | 200 | **unadjudicable + no-op** |
| C2 terminal | 28.6% | 20% | 59.5% | 179 | **KILLED** |

- **I1** — PRIMARY predict +8pt: FAIL (moved down, but meaningless). GUARD restatement ≥71%: PASS (76.2%); ungrounded-implication: N/A.
- **I2** — PRIMARY attention-eff +8pt: UNADJUDICABLE (not instrumented). GUARD restatement: PASS (81.0%).
- **S1** — PRIMARY attn-eff + p95: UNADJUDICABLE. GUARD restatement: PASS (85.7%); canaries survive: PASS (3/3).
- **C1** — PRIMARY status-label accuracy ≥80%: UNADJUDICABLE. GUARD restatement: borderline (−16.7pt).
- **C2** — PRIMARY noun-grounding +15pt: FAIL (+7.2pt, in noise); trajectory: N/A. GUARD restatement: **FAIL (−26.2pt)** → GLOBAL KILL.

## The three load-bearing findings

1. **Inject arms (I1/I2/S1) are structurally untestable on this dispatch — a harness wiring gap, not corpus thinness.** `eval_run13::b0_claims_briefing` hardcodes its own librarian system prompt and never calls `inject::compile_preamble()`/`select_preamble()` (grep-confirmed: `eval_run13.rs` references neither). The toggle is set process-wide but no scored code reads it. **Empirical proof:** the four inject runs are the *same computation*, yet B0 grounding swung **21.4→35.7→14.3→14.3%** — a **±21pt** judge-noise spread, with predict any-signal ±20pt and restatement ±10pt. The noise floor is larger than every +8/+15pt bar.

2. **C1 typed is doubly inert.** `ClaimRecord` (src/claims.rs) has no `status` field, so serde drops the typed label at persistence (rebuilt store-b had **0** status-tagged claims), and run13 has no status-accuracy instrument. The 24 status fixtures are validated for count but never scored.

3. **C2 terminal is a real kill.** Its replacement-mandate consolidation cut the store **260→179 claims (−31%)** and collapsed B0 restatement recall **85.7%→59.5%** — a fact-coverage regression that trips the restatement guard. Its noun-grounding "gain" (+7.2pt) is below bar and inside noise.

**Wins to land default-on: none.** **Kill: C2 terminal.** **Unreadable (harness gaps): I1/I2/S1, C1.** **Too thin even where instrumented:** C2 noun-grounding sits inside the measured ±21pt noise band.

Recommendations for a re-run are in the doc: route B0 compilation through the real inject preambles + add SELECT/attention-eff/p95 instruments (inject), add a `status` schema field + fixture status-scorer (C1), and grow the probe — the current n=14/20/42 noise floor is below the pre-registered effects.

Key paths:
- `/Users/pablofernandez/src/proactive-context/.claude/worktrees/agent-a5389aaa300fea689/docs/product-spec/prompt-variant-results.md`
- `/Users/pablofernandez/src/proactive-context/.claude/worktrees/agent-a5389aaa300fea689/docs/product-spec/prompt-variant-artifacts/` (per-arm logs + scoring jsonl + store-b claim counts)

A process note: I worked in the `agent-a5389aaa300fea689` worktree on `master` and committed there (not pushed/merged). Given the known multi-agent branch-GC hazard, you may want to merge this increment promptly.
