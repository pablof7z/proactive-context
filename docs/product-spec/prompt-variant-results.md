# Prompt-Variant A/B Results (pc/cfv6, within-run)

**Date:** 2026-06-16
**Spec:** `/tmp/prompt-variant-spec.md` (Opus prompt-designer); arms wired in `src/eval_prompt_variant.rs`, preambles in `src/inject.rs` (COMPILE/SELECT) + `src/capture.rs` (EXTRACT).
**Runner:** `pc eval --prompt-variant <arm> --experiment-dir <dir> --project <pc repo>`
**Corpus:** pc/cfv6 ONLY — frozen experiment dir `~/.proactive-context/experiments/cfv6-20260611-012408` (HISTORY=30 sessions, 14 frozen idiosyncratic noun-moments, 20 verified corrections, 42 restatement labels).
**Models:** generative + judge = `ollama:glm-5.1:cloud` (think-ON, $0, `PC_RUN13_RETRY=5`). Detector `PC_RUN13_DETECT_TURN_CAP=120`.
**Method:** each arm run in its own isolated dir (frozen inputs symlinked; `run13_arms/predict/p1.jsonl` regenerated per arm so no cross-arm cache bleed; frozen `run13_nouns.jsonl` shared so the probe is identical across arms). Capture arms (C1/C2) ran against a store-b **rebuilt under the EXTRACT toggle** over the same 30 HISTORY sessions; C0 baseline = the frozen base store-b. Artifacts in `docs/product-spec/prompt-variant-artifacts/`.

---

## TL;DR

- **No arm is a win worth landing default-on.** None cleared its pre-registered PRIMARY with guards intact.
- **C2 terminal: KILLED** (restatement GUARD fail — fact-coverage regression).
- **I1 / I2 / S1 (all inject arms): UNADJUDICABLE — harness wiring gap.** The dispatched run13 bundle compiles its briefing with a *hardcoded librarian prompt* and never reads `PC_COMPILE_VARIANT` / `PC_SELECT_VARIANT`. The toggle is set process-wide but no code on the scored path consumes it. Confirmed by code **and** empirically (4 identical-by-construction inject runs swung ±21pt).
- **C1 typed: UNADJUDICABLE + pipeline no-op.** The `status` field is emitted by the prompt but dropped at persistence (`ClaimRecord` has no `status` field, serde discards it) AND there is no status-accuracy instrument. Doubly inert.
- **Corpus is too thin for the pre-registered effect sizes.** The four identical inject runs establish a judge-noise floor of **±21pt (noun-grounding), ±20pt (predict any-signal), ±10pt (restatement)** on this n=14/n=20/n=42 corpus — *larger than every +8pt / +15pt bar*. Even the one genuinely-movable signal (C2 noun-grounding, +7.2pt) sits inside that noise band.
- Seeded **noun** canaries (`capture-pipeline`, `context-injection`, `reranking`) recovered **3/3 in every arm**. The **status / reversal / default-flip / F8-trap** fixture families are validated for *count* by the runner (`assert_canary_bars`) but **never scored** — no instrument consumes them.

---

## Raw numbers (B0 column = what each toggle can move)

The runner dispatches the Run-13 noun-primer bundle. For the *prompt-variant* question, the load-bearing column is **B0** (the plain briefing): inject toggles would change how B0 is compiled; capture toggles change the store B0 reads from. The A1/A2/A3 primer arms and the run13 internal CONFIRMED/REJECTED verdict are about the *noun-primer* experiment and are **not** the prompt-variant signal.

| Arm | toggle | store-b (claims) | B0 noun-grounding | B0 predict (pred / partial; any-signal) | B0 restatement P1 |
|-----|--------|------------------|-------------------|------------------------------------------|-------------------|
| **I0 = C0 baseline** | librarian / base | frozen (260) | 21.4% | 1/20 ; 5/20 (30%) | 36/42 = 85.7% |
| I1 verdict | COMPILE=verdict | frozen (260) | 35.7% | 1/20 ; 1/20 (10%) | 32/42 = 76.2% |
| I2 divergence | COMPILE=divergence | frozen (260) | 14.3% | 2/20 ; 3/20 (25%) | 34/42 = 81.0% |
| S1 select-verdict | SELECT=verdict | frozen (260) | 14.3% | 0/20 ; 4/20 (20%) | 36/42 = 85.7% |
| C1 typed | EXTRACT=typed | rebuilt (200) | 14.3% | 1/20 ; 4/20 (25%) | 29/42 = 69.0% |
| C2 terminal | EXTRACT=terminal | rebuilt (179) | 28.6% | 1/20 ; 3/20 (20%) | 25/42 = 59.5% |

**Noise floor (decisive):** I0/I1/I2/S1 are the *same underlying computation* — same frozen store-b, same hardcoded-librarian B0 compile (toggle unread). Their spread is therefore pure judge/generation nondeterminism:
- B0 noun-grounding: 14.3 – 35.7% → **±21pt**
- B0 predict any-signal: 10 – 30% → **±20pt**
- B0 restatement P1: 76.2 – 85.7% → **±10pt**

---

## Pre-registered adjudication matrix (verbatim PASS/FAIL)

### I1 verdict — PRIMARY predict-the-correction +8pt vs I0; GUARD restatement ≥71%, ungrounded-implication = 0
- PRIMARY: **FAIL / UNADJUDICABLE.** B0 predicted 1/20 vs I0 1/20 = +0; any-signal 10% vs 30% = −20pt (moved *down*). But the result is meaningless: `eval_run13::b0_claims_briefing` hardcodes its own librarian system prompt and never calls `inject::compile_preamble()`, so `COMPILE=verdict` had no effect. The IMPLICATION line the arm is supposed to add is never generated.
- GUARD restatement ≥71%: **PASS** (76.2%). ungrounded-implication = 0: **N/A** (no implication line produced on this path).
- **Verdict: NULL — not testable on the dispatched harness.**

### I2 divergence — PRIMARY attention-efficiency +8pt (full or +5pt implicit subset); GUARD restatement, trajectory floor
- PRIMARY: **UNADJUDICABLE.** run13 emits no attention-efficiency *delta* — "load-bearing subset" is a mining-derived partition (all 14 moments load-bearing), independent of the briefing, so a COMPILE variant cannot move it. (And `COMPILE=divergence` is unread anyway.) B0 grounding 14.3% vs I0 21.4% = −7.1pt (inside ±21pt noise).
- GUARD restatement: **PASS** (81.0%). trajectory floor: **N/A** (not instrumented).
- **Verdict: NULL — not testable.**

### S1 select-verdict — PRIMARY attn-eff + p95 latency; GUARD restatement, seeded canaries survive
- PRIMARY: **UNADJUDICABLE.** run13 retrieves claims via `claims::retrieve_top_clusters` with no SELECT stage, so `inject::select_preamble()` is never invoked; and neither attention-efficiency nor p95 latency is emitted by this bundle.
- GUARD restatement: **PASS** (85.7%). seeded canaries survive: **PASS** (3/3 noun canaries recovered).
- **Verdict: NULL — not testable.**

### C1 typed — PRIMARY status-label accuracy ≥80% with settled-as-proposed ≤10% on seeded fixtures
- PRIMARY: **UNADJUDICABLE (doubly inert).** (1) The typed prompt emits a `status` field, but `ClaimRecord` (src/claims.rs) has **no `status` field** — serde silently drops it; the rebuilt store-b/claims.jsonl contains **0** status-tagged claims. (2) run13 has **no status-accuracy instrument**; the 24 status fixtures (12 settled / 12 proposed) are validated for *count* only. Adjudicating C1 requires a dedicated label-separability probe (feed the 24 fixtures through typed-EXTRACT, parse status from the raw response) that does not exist.
- GUARD restatement (no facts dropped): B0 P1 69.0% vs baseline 85.7% = −16.7pt; claims 200 vs 260. Borderline — within ~2× noise but with a mechanism (fewer claims). Not clean.
- **Verdict: UNADJUDICABLE — and currently a pipeline no-op.**

### C2 terminal — PRIMARY trajectory+stale-leak AND noun-grounding +15pt zero-drift; GUARD restatement, attn-eff (bloat)
- PRIMARY noun-grounding +15pt: **FAIL.** C2 B0 28.6% vs C0 21.4% = **+7.2pt** (< +15pt, and inside the ±21pt noise band). trajectory + stale-leak: **N/A** (not instrumented by this bundle).
- GUARD restatement: **FAIL.** C2 B0 P1 **59.5%** vs C0 85.7% = **−26.2pt** (also −16.7pt vs the noisiest inject run). Mechanism: the terminal variant's replacement-mandate consolidation cut the store from **260 → 179 claims (−31%)**, dropping facts the restatement probe needs. attn-eff (bloat): N/A.
- **Verdict: KILLED — GLOBAL KILL on the restatement guard (fact-coverage regression), PRIMARY also not met.**

---

## Wins / Kills / Too-thin

- **WINS worth landing default-on:** **NONE.**
- **KILLED:** **C2 terminal** — restatement guard fail (−26pt B0 P1, −31% claim coverage). The replacement-mandate trades fact recall for consolidation; net negative on this corpus. Do **not** land default-on.
- **UNADJUDICABLE — harness/instrument gap (NOT corpus thinness):**
  - **I1, I2, S1** — the dispatched bundle never reads the COMPILE/SELECT toggles (hardcoded-librarian B0 compile; no SELECT stage; no attention-efficiency / p95 instruments). These arms cannot be tested until the harness routes B0 compilation through `inject::compile_preamble()` / `select_preamble()` and emits the attention-efficiency + latency instruments.
  - **C1 typed** — `status` neither persisted (`ClaimRecord` lacks the field) nor scored (no status-accuracy instrument). Needs both a schema field and a label-separability probe.
- **CORPUS TOO THIN to adjudicate (even where instrumented):** the only genuinely-movable signal, **C2 noun-grounding (+7.2pt)**, lands inside the ±21pt judge-noise floor that the four identical inject runs measured directly. On n=14 grounding / n=20 predict / n=42 restatement, a single within-run cannot resolve the pre-registered +8/+15pt effects.

---

## Recommendations

1. **Land nothing default-on from this round.** C2 is a clear kill; the rest are untested.
2. **Fix the inject harness before re-running I1/I2/S1.** Route `eval_run13::b0_claims_briefing` (and the predict/P1 ride-along briefings) through the real `inject::compile_preamble()` so `PC_COMPILE_VARIANT` actually changes B0; add a SELECT stage for S1; emit an attention-efficiency *delta* and p95 latency. Until then these arms produce I0±noise.
3. **C1 needs a schema + probe.** Add `status` to `ClaimRecord` (and the EXTRACT parser) so the label survives persistence, then a fixture-driven status-accuracy scorer over the 24 seeded fixtures. Only then is C1's PRIMARY readable.
4. **Grow the probe before chasing +8pt.** With a ±20pt single-run noise floor, detecting an +8pt prompt effect needs either many more moments/corrections or paired/seeded repeated-judge designs. The pre-registered bars are below the current noise floor.
5. **Score the seeded canary families.** reversal / default-flip / F8-trap / status fixtures are counted but never scored; their guards (e.g. "seeded canaries survive the gate", "settled-as-proposed ≤10%") can't be rendered until an instrument consumes them.

---

## Artifacts

`docs/product-spec/prompt-variant-artifacts/`:
- `<ARM>.log` — full console report per arm (grounding table, ride-alongs, bars, verdict, canary recovery).
- `<ARM>_run13_arms.jsonl` / `_run13_predict.jsonl` / `_run13_p1.jsonl` — per-arm regenerated scoring rows.
- `store_b_claim_counts.txt` — baseline 260 vs C1 200 vs C2 179 (the capture-toggle store deltas).
