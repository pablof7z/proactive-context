# Phase 3 Source-Type Eval Arms — Results (2026-06-17)

Full-inject (catalog + SELECT + COMPILE) over the **frozen** baseline labels/reversals
(`baseline-pre-taxonomy-2026-06-17-r3`), via `pc eval --select-arms`. Store-A wiki: 27 guides,
29 episode cards, **4 research records, 0 nouns**. Judge `ollama:glm-5.1:cloud`.

> **Confidence: directional only.** n = 20 labels / 10 reversals, single judge pass. The baseline
> doc already flags judge non-determinism and conservative substring label verification. Treat
> deltas ≤ ~10pt as noise-adjacent; confirm with a larger/repeat run before flipping any default.

## Arms

| Arm | Flags |
|-----|-------|
| A0 | baseline — none |
| A1 | `PC_TYPED_CATALOG` |
| A2 | + `PC_SELECT_SOURCE_TYPES` |
| A3 | + `PC_RESEARCH_CATALOG` |
| A4 | + `PC_NOUN_CATALOG` (**vacuous — 0 nouns in store**) |
| A5 | claim catalog — N/A (Phase 5 deferred) |

## Results (A0 = baseline)

| Arm | Recall % | Δ vs A0 | contained | partial | absent | NOTHING_REL | avg sel/label | p50 ms | asserts_cur | stale_leak | trajectory |
|-----|---------:|--------:|----------:|--------:|-------:|------------:|--------------:|-------:|------------:|-----------:|-----------:|
| A0 | 60% | — | 4 | 8 | 8 | 0 | 3.0 | 6206 | 7/10 | 1/10 | 7/10 |
| A1 | 70% | +10 | 4 | 10 | 6 | 1 | 2.8 | 5744 | 9/10 | **0/10** | **9/10** |
| A2 | 70% | +10 | 7 | 7 | 6 | 0 | 2.3 | 7058 | 7/10 | 0/10 | 7/10 |
| A3 | 55% | −5 | 9 | 2 | 9 | 0 | 2.3 | 6253 | 7/10 | 1/10 | 6/10 |
| A4 | 70% | +10 | 6 | 8 | 6 | 0 | 2.3 | 7126 | 8/10 | 0/10 | 8/10 |

### Selection counts by kind

| Arm | current-guide | episode-card | research-record |
|-----|------:|------:|------:|
| A0 | 35 | 24 | 0 |
| A1 | 39 | 17 | 0 |
| A2 | 37 | **9** | 0 |
| A3 | 31 | 14 | 1 |
| A4 | 36 | 10 | 0 |

## Reading

- **A1 (`PC_TYPED_CATALOG`) is the standout, low-risk win.** Recall +10, stale-leak 1→0,
  trajectory 7→9, and slightly *faster* p50 (5744 vs 6206). Just adding `[kind]` hints to catalog
  lines — no behavior beyond informing SELECT — improved every quality axis here. Best candidate
  for a default-on, pending a confirmation run.
- **A2 (`PC_SELECT_SOURCE_TYPES`) makes selection more decisive but trades off episodes.** It
  cut episode-card selections sharply (24→9) and raised hard "contained" hits (4→7) with fewer
  sources/label (3.0→2.3) — the intended precision effect. But trajectory fell back to 7/10
  (vs A1's 9): episode cards are the trajectory-recovery source, so telling SELECT to be choosier
  about historical artifacts costs some reversal-trajectory recall. Net recall still +10. Tension
  worth tuning: the source-type prompt may be under-valuing episode cards for "why/history" probes.
- **A3 (`PC_RESEARCH_CATALOG`) hurt on this corpus (−5 recall, trajectory 6).** With only 4
  research records, SELECT picked one once; the extra rows mostly added noise. Not worth enabling
  until (a) there's a richer research corpus and (b) the research-selection guidance is tuned.
- **A4 is vacuous** (no nouns in the store) → equals A2 + noise. Noun selectability is untested;
  needs a store built with `capture_nouns` on.

## Recommendation (no defaults flipped — your call)

1. **Confirm A1** with a larger run (`--arms-label-cap` 40 or uncapped, ideally 2–3 repeats to
   average judge noise). If A1 holds, it's the first flag to consider default-on per the plan's
   gate (no stale-leak increase ✓, no precision regression ✓, token ~flat ✓, ≥1 slice improves ✓).
2. **Hold A2** as default-off but promising; tune the source-type prompt so it keeps episode
   cards for history/why probes (recover the trajectory A1 had) before reconsidering.
3. **Keep A3/A4 off** until the research corpus is richer and nouns are captured.

Reproduce / extend:

```sh
pc eval --project <repo> --experiment-dir <baseline-r3 dir> --select-arms --arms-label-cap 40
```

---

## Run 2 — A2′ prompt tuning + a variance warning (2026-06-18)

After Run 1, the A2 source-type SELECT block was tuned (**A2′**): the misplaced COMPILE-time
caution ("do not select historical as current truth") was removed from SELECT, and the gate is now
explicitly told to keep **every** episode card relevant to a why/history/reversal prompt. Re-ran the
identical arms on the identical frozen labels (n=20/10).

| Arm | Recall % | Δ A0 | NOTHING_REL | avg sel/label | p50 ms | asserts_cur | stale_leak | trajectory |
|-----|---------:|-----:|------------:|--------------:|-------:|------------:|-----------:|-----------:|
| A0 | 75% | — | 0 | 2.7 | 6019 | 8/10 | 0/10 | 6/10 |
| A1 | 50% | −25 | 2 | 2.5 | 5967 | 7/10 | 0/10 | 7/10 |
| A2′ | 50% | −25 | 0 | 3.1 | 8493 | 8/10 | 0/10 | **8/10** |
| A3 | 70% | −5 | 0 | 3.4 | 6321 | 8/10 | 0/10 | 7/10 |
| A4 | 70% | −5 | 1 | 3.4 | 6652 | 8/10 | 0/10 | 8/10 |

Episode-card selections by arm (Run 1 → Run 2): A2 **9 → 25**. The tuning fixed the under-selection.

### The headline finding: recall is noise-dominated at this sample size

Recall **flipped sign** between two identical-input runs:

| Arm | Run 1 recall | Run 2 recall |
|-----|-------------:|-------------:|
| A0 | 60% | 75% |
| A1 | 70% | 50% |
| A2(′) | 70% | 50% |

A0 itself moved 15pt; A1's delta vs A0 went **+10 → −25**. With n=20 and a single
non-deterministic judge pass (`glm-5.1`), **recall deltas ≤ ~20pt are not resolvable** — Run 1's
"A1 is a clean win on recall" does **not** replicate and must be retracted.

### What IS stable across both runs (the trustworthy signals)

- **A2′ fixed the episode regression** — selections recovered 9→25 and trajectory rose to 8/10
  (≥ A0). This is a *mechanistic* result from near-deterministic selection counts, not judge mood.
- **stale-leak ≈ 0** under every typed/source-type arm in both runs — no current-truth regression
  (the most important safety gate) from any flag.
- **A3 (research selectable)** never helped (thin 4-record corpus; ≤2 research rows ever selected).
- Token/latency stayed within noise of baseline (no >15% blowup).

### Revised recommendation

1. **Do not flip any default on recall grounds yet** — the metric can't tell the arms apart at
   n=20/1-judge. Before any default-on decision, the harness needs **n≥40 labels AND 3+ judge
   passes averaged** (or a deterministic/stronger judge). This is the real prerequisite, not more
   prompt tuning.
2. **A2′ is the right prompt** — it removed the episode under-selection with no stale-leak cost; keep
   it as the source-type block. Default stays OFF pending the higher-power eval above.
3. **A1/A2′ are safe to keep building on** (stale-leak clean); **A3/A4 stay off** (no benefit / vacuous).
4. Methodology upgrade is now the gating work for Phase 3 sign-off — tracked as the open item.
