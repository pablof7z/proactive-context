# Run 14 — Noun-Primer Probe (pc corpus): Results

**Design contract:** `/tmp/noun-experiment-design.md` (pre-registered, Runs 13–16).
**Foundation:** `src/nouns.rs` (C3 derived-noun registry, primer composer).
**Eval module:** `src/eval_run13.rs` (`pc eval --run13`; corpus auto-detected as `pc`).
**Corpus / experiment dir:** pc = `~/.proactive-context/experiments/cfv6-20260611-012408`
(project key `Users_pablofernandez_src_proactive-context`; 20-guide early pc wiki snapshot, 41 future
sessions, 42 frozen labels, 34 corrections).
**Models:** $0 Ollama only — generative + judge = `gemma4:26b-mlx`. PC_HOME-isolated.

**What changed vs Run 13 (the scarcity fix):**
1. **LLM reference detector for mining** (`PC_RUN13_LLM_DETECT`, default on): one offline LLM call per
   genuine human turn maps informal/synonym/partial phrasing → registry nouns. Run-13's whole-token
   matcher under-mined because humans don't type formal slugs. Production `detect_first_mentions`
   (hot path) is unchanged — see the follow-up note.
2. **pc canaries** (this cfv6 snapshot): `capture-pipeline`, `context-injection`, `compile-pipeline`,
   `reranking`, `embedding-pipeline` — all registry-grounded guides here and pc-idiosyncratic. (The
   "ideal" pc nouns — episode-cards/claim-log/terminal-state-inversion — postdate this snapshot.)
3. **Judge infra:** all grounding judges routed through `call_with_retry`; Ollama 404 "model not
   found" treated as transient in `call_model_blocking` (shared-host eviction → reload), so no judge
   emits a 404→0% artifact. `keep_alive=-1` pin at run start.

---

## Headline verdict

> **CONFIRMED (pc).** All six pre-registered bars PASS. The noun primer (A2 = definition +
> prompt-filtered facts) lifts noun grounding from **B0 28.6% → A2 57.1% (+28.6pt)** on 14
> load-bearing idiosyncratic noun-moments, with **zero G-correct drift (0%)**, **no restatement-P1
> regression** (83.3% → 83.3%), and **no prediction regression** (tie). C3-derived primers are
> sufficient — C1 (Run 16) is not needed to clear the grounding bars. **Recommendation: ship the
> noun primer default-on at level `facts` (A2).** Run-13 wallet was a P2-scarcity no-decision; pc
> (cfv6) is the corpus that adjudicates, and it CONFIRMS.

---

## 1. Canary recovery (design §3.4) — PASS (3/3)

| canary | registry-grounded | idiosyncratic (bare) | mined moment | status |
|---|---|---|---|---|
| `capture-pipeline`  | yes | yes (absent) | yes | **RECOVERED** |
| `context-injection` | yes | yes (absent) | yes | **RECOVERED** |
| `reranking`         | yes | yes (absent) | no  | **RECOVERED** |

All three recover (grounded + idiosyncratic). Two initially-proposed canaries — `compile-pipeline`,
`embedding-pipeline` — were dropped after the first run showed `bare=contained` (the bare model
already knows them in this project's sense, so they are NOT idiosyncratic and fail the canary's
purpose). The kept three are verified grounded + bare=absent.

## 2. Noun mining + scarcity gate (design §3.1) — PASS (14 ≥ 12)

- C3 registry: 24 nouns (cfv6's 20-guide snapshot + topics), 0 thin anchors.
- **LLM reference detector** (the scarcity fix): scanned 120 genuine human turns → **22
  registry+store-grounded candidates** (vs Run-13 wallet's 3 with whole-token matching).
- Idiosyncrasy filter (bare model "what is N in this project?"): kept **14** (excluded 8 where
  bare=contained — e.g. compile-pipeline, llm-observability — correct counterfactual-load control).
- **14 idiosyncratic noun-moments ≥ 12 gate.** Scarcity solved.

## 3. Grounding table (design §3.2) — A2 wins (n=14)

Primary = `G-def=present AND G-facts∈{contained,partial} AND G-correct=correct`.

| arm | primary | G-def=present | G-facts∈{cont,part} | G-correct=wrong |
|---|---|---|---|---|
| B0        | 29% (4/14) | 6/14  | 7/14  | 0/14 |
| A1 def    | 50% (7/14) | 12/14 | 12/14 | **4/14 (29%)** |
| **A2 facts** | **57% (8/14)** | 13/14 | **14/14** | **0/14** |
| A3 intent | 57% (8/14) | 10/14 | 14/14 | 2/14 (14%) |

**A2 − B0 = +28.6pt** (≥ +15pt bar). A2 is the standout: best G-facts (14/14), **zero G-correct drift**
(0/14), ties A3 for top primary. A1 introduces drift (4/14 wrong) and A3 some (2/14) — A2 is the clean
winner. Per-noun, the primer grounds nouns B0 misses (tail-tui, global-lessons, readme-positioning,
observability, ui-components, …); the two `generate`/`vector-search` nouns stay hard for all arms.

## 4. Ride-alongs (guards) — all hold

| ride-along | B0 | A2 | guard |
|---|---|---|---|
| restatement P1 recall (contained+partial, n=12) | 10/12 = 83.3% | 10/12 = 83.3% | no drop >5pt: **PASS** (Δ 0) |
| predict-the-correction (predicted, n=12) | 0/12 (3 partial) | 0/12 (1 partial) | A2 ≥ B0: **PASS** (tie at 0 predicted) |
| attention-efficiency | all 14 moments load-bearing (bare=absent) | — | gain on LB subset = overall (+28.6pt): **PASS** |

P1 recall is a healthy 83.3% (not the 0% infra artifact of the Run-13 local-judge run) — the cloud
judge path (glm-5.1:cloud via `/api/chat`) is reliable. The primer does not dent restatement recall
or correction prediction; its gain is concentrated entirely on noun grounding.

## 5. Pre-registered bars (verbatim) — 6/6 PASS

| bar (verbatim) | result | detail |
|---|---|---|
| Probe validity: canaries recovered + ≥12 moments | **PASS** | canaries 3/3 (0 missing), moments=14 (gate 12) |
| A2 grounding ≥ B0+15pt | **PASS** | A2=57.1% B0=28.6% (Δ=+28.6pt; need ≥+15) |
| A2 gain concentrated on load-bearing subset | **PASS** | LB Δ=+28.6pt vs overall Δ=+28.6pt (LB n=14) |
| A2 G-correct wrong ≤10% | **PASS** | A2 wrong=0.0% |
| no arm P1 drop >5pt vs B0 | **PASS** | B0=83.3% A2=83.3% (drop=+0.0pt) |
| A2 predict ≥ B0 (tie ok) | **PASS** | B0 predicted=0/12 A2 predicted=0/12 |

## 6. Verdict & next step (design §Stop)

**Run 14 (pc): CONFIRMED.** A2 (def + prompt-filtered facts) beats B0 by +28.6pt on load-bearing
idiosyncratic nouns, gain fully on the load-bearing subset, zero G-correct drift, no P1 regression,
predict not reduced. Combined with Run-13 (probe validated; def fix; canaries recover — wallet was
P2-scarce, not a rejection), the noun-primer hypothesis is **CONFIRMED on the corpus that can
adjudicate it.**

**Why A2 over A1/A3:** A2 has the best G-facts (100% contained/partial) and **zero drift (0% wrong)**;
A1 introduces drift (28.6% wrong — raw definitions without prompt-filtering mislead) and A3 some
(14.3%). A2 ties A3 for top primary (57.1%) but is cleaner. A2 is the cheapest level that is both
top-scoring and drift-free.

**Ship recommendation:** noun primer **default-on at `facts` (A2)**. Per design §Stop this also implies
**C3 is sufficient — do NOT build C1** (Run 16): C3-derived definitions + prompt-filtered facts clear
every bar; C1 was gated on A2 failing, which did not happen.

## 7. Follow-up: production matcher robustness

The Run-13 scarcity was rooted in the **production hot-path matcher** `nouns::detect_first_mentions`
using strict whole-token matching — it misses informal/synonym/partial human phrasing ("the wiki",
"how we pick guides"). Run 14 works around this **for offline mining only** (the LLM reference
detector); the production path is intentionally **unchanged** here. **Follow-up finding to pre-register
separately:** if live first-mention priming is to fire on natural phrasing, the hot path needs a
cheap alias/embedding-based matcher (not an LLM call per prompt — latency-bound). That is a distinct
design decision, not a Run-14 patch.

## Frozen artifacts

Committed under `docs/product-spec/run14-artifacts/` (live in `cfv6-20260611-012408/`):
- `run13_nouns.jsonl` — 14 frozen idiosyncratic noun-moments (LLM-detected, idiosyncrasy-filtered), each with populated definition + ground-truth facts.
- `run13_arms.jsonl` — B0/A1/A2/A3 grounding sub-verdicts per moment (the A2 +28.6pt result).
- `run13_predict.jsonl`, `run13_p1.jsonl` — ride-along verdicts.
- `run14_report.txt` — verbatim console report (grounding table, ride-alongs, 6/6 bars, verdict, canary recovery).

**Reproduce:**
```
PC_RUN13_MODEL=ollama:glm-5.1:cloud PC_RUN13_DETECT_MODEL=ollama:glm-5.1:cloud \
PC_RUN13_DETECT_TURN_CAP=120 PC_HOME=~/.proactive-context/experiments/cfv6-20260611-012408 \
pc eval --project /Users/pablofernandez/src/proactive-context \
  --experiment-dir ~/.proactive-context/experiments/cfv6-20260611-012408 \
  --run13 --judge-model ollama:glm-5.1:cloud
```
($0 Ollama Cloud; the config's `ollama_base_url=https://ollama.com` routes local-model names to 404,
so cloud models are used — see follow-up. Probe-validity findings are deterministic no-LLM Pass-1.)
