# T-A — Noun-Realness Scorer Bake-off (RESULTS)

**Winner: Approach A — signed-delta ledger.** Land A default-on as the surfacing gate; keep C as a
free interpretability layer over A's events; keep B as an earned fallback. This is exactly the prior
both independent designs predicted ("A cheapest, C free layer over A, B fallback").

Builds directly on **T-0 PASS** (the stance classifier reads the user's stance reliably on the
sign-determining axis: reject-precision 1.000, zero reject↔operate_on confusions). T-A asks the next
question: *given* reliable per-reference stance, which AGGREGATION of those events into a per-noun
realness verdict best separates REAL project nouns from REJECTED confabulations — cheaply, safely, and
stably?

---

## What was built

All three scorers from `/tmp/realness-plan.md`, flagged, each consuming the **same thinking-ON
batched stance reads** over a noun's USER-turn references (`src/realness.rs`):

- **A — signed-delta ledger** (`score_ledger`): realness = signed sum of per-reference deltas
  (`operate_on +1`, `reject −2`, `neutral 0`). Thresholds **real ≥ +3 / suppress ≤ −2**. Recovery is
  automatic (order-independent sum). Cost model = one batched stance call per session.
- **B — holistic re-judgment** (`judge_holistic`): one LLM call per noun over its WHOLE user-reference
  history → `{status, score∈[−1,1]}`.
- **C — lifecycle state-machine** (`run_lifecycle` + `apply_dormancy`): the same stance events as A,
  discretized into **Candidate / Provisional / Real / Rejected / Dormant** with hysteresis (promotion
  is sticky; Rejected only recovers on a clear +1 climb; stale reals decay to Dormant).
- **Frequency-only baseline**: rank by mention count (the "more references → more real" hypothesis,
  unsigned).

Both T-0 carry-forward findings were honored:

1. **Entity-candidate filter** (`eval_realness::is_entity_candidate`, unit-tested): drops file:line
   refs (`message.rs:413`), code/JSON/snippet punctuation, `Foo::bar`, multi-dot attribute access,
   hex hashes, mid-colon heading fragments, transcript/role artifacts; keeps real project nouns
   (named components, NIPs, commands, files-as-entities the user names).
2. **Parse-repair** in `classify_batched`: any item the model omits/truncates is re-asked in
   successively smaller sub-batches (down to one ref). **Drops = 0 in every run.**

`pc eval --realness` runs the bake-off; `PC_REALNESS_MINE=1` dumps the population for curation.

---

## Pre-registered bars (verbatim, declared in code before scoring)

- **Separation**: each approach's REAL-vs-REJECTED **AUC ≥ 0.85** AND **beats the frequency baseline
  by ≥ 0.10 AUC**.
- **Reject-precision ≥ 0.90**: of the nouns an approach promotes to real, ≥90% are not gold-rejected
  (never prime a confabulation).
- **Recovery**: a rejected-then-operated-on noun climbs back above threshold (promoted to real).
- **Determinism**: ≤ **10%** verdict-flip across 2 runs (esp. B).
- **Cost**: report LLM calls + tokens per approach.

> A post-hoc but decisive **promotion-precision** metric was added after the per-noun data exposed an
> over-priming failure the pre-registered bars missed (see "The decisive metric" below). The
> pre-registered bars are reported as-is; promotion-precision is disclosed as an additional,
> winner-selecting gate.

---

## Gold set (frozen, committed)

`docs/product-spec/realness-artifacts/gold_nouns.jsonl` — **34 nouns** (26 curated cfv6 + 8
hand-seeded canaries), each embedding its references so re-scoring never re-mines.

- Distribution: **real 12 · neutral 18 · rejected 4.**
- **Corpus waiver** (Pablo, overrides the two-corpora default): **pc/cfv6 ONLY**; wallet/cfv3 is not a
  real project and is excluded from all realness experiments. Single-corpus is intentional.
- Canaries supply the naturally-rare REJECTED class plus the recovery and dormant cases: the brief's
  `fabric-provider` reject, `SyncOrchestrator`, `RetryDaemon` (rejects); `context injection`,
  `capture pipeline`, `archeologist feature` (reals, the last one stale → dormant); `vector database`
  (neutral); and **`episode cards`** — the **recovery canary** (doubted, then adopted and operated on).
- A natural reject was mined from cfv6: **`pc autodoc`** — *"what is this `pc autodoc` command? it
  writes some bullshit guide???!"*
- The NEUTRAL class deliberately includes **pasted code-symbol noise** (`encode_ask`, `is_tool_use`,
  `tool_name`, `handle_thread_event`, …) — exactly the junk finding #1 warns about. Labeling it
  NEUTRAL turns it into a live test of whether a scorer wrongly PRIMES snippet noise.

---

## Results (run-0 canonical; determinism vs run-1; glm-5.1:cloud, think-ON, $0)

| approach | AUC | reject-prec | **promotion-prec** | real-recall | recovery | flip% | LLM calls | ~tokens | promoted real/neut/rej |
|---|---|---|---|---|---|---|---|---|---|
| **A signed-delta ledger** | **1.000** | **1.000** | **1.000** | 0.333 | ✅ | **3%** | **22** | 21 000 | **4 / 0 / 0** |
| B holistic re-judgment | 1.000 | 1.000 | 0.588 | 0.833 | ✅ | 15% | 34 | 30 477 | 10 / 7 / 0 |
| C lifecycle state-machine | 1.000 | 1.000 | 1.000 | 0.250 | ✅ | 32% | 22 (shared) | 21 000 | 3 / 0 / 0 |
| frequency baseline | 0.500 | 0.800 | 0.800 | 0.333 | ✅ | 0% | 0 | 0 | 4 / 0 / 1 |

`reject-precision` = of promoted, fraction NOT rejected. `promotion-precision` = of promoted, fraction
genuinely REAL (penalizes priming NEUTRAL noise too). `real-recall` = of gold-real nouns, fraction
promoted.

### Per-bar verdicts (all four bars, verbatim)

| bar | A | B | C |
|---|---|---|---|
| Separation: AUC ≥ 0.85 AND ≥ freq+0.10 | **PASS** (1.000 vs 0.500) | PASS (1.000) | PASS (1.000) |
| Reject-precision ≥ 0.90 | **PASS** (1.000) | PASS (1.000) | PASS (1.000) |
| Recovery (episode cards → real) | **PASS** | PASS | PASS |
| Determinism ≤ 10% flip | **PASS** (3%) | **FAIL** (15%) | **FAIL** (32%) |
| Cost (LLM calls / run) | **22 (shared)** | 34 (per-noun) | 22 (shared) |
| Promotion-precision ≥ 0.90 (added gate) | **PASS** (1.000) | **FAIL** (0.588) | PASS (1.000) |

---

## The headline finding: frequency is chance; **direction**, not count, makes a noun real

The frequency baseline scores **AUC 0.500 — pure chance** — and it promotes `fabric-provider`
(gold-REJECTED, mentioned 3×) as "real". Pablo's own hypothesis in the corpus was *"a noun becomes
real when the user references it; the more the user references it, the more real it becomes."* The data
refines it: **raw reference count cannot tell an owned noun from a disowned one** (you disown a thing
by talking about it). It is the **signed direction** of those references — the stance axis T-0 proved
readable — that separates real from rejected. Every stance-based scorer (A/B/C) jumps from 0.500 to
**1.000** AUC. That gap is the whole value of the realness model.

## The decisive metric: promotion-precision (over-priming guard)

On the headline real-vs-rejected AUC all three stance scorers **tie at 1.000** — the stance axis is so
clean that separation is "solved". The real decision is made by the safety/stability metrics, and the
per-noun detail exposed a failure the pre-registered bars missed:

- **B over-promotes NEUTRAL pasted code-symbol noise.** It labels `encode_ask`, `is_tool_use`,
  `tool_name`, `handle_thread_event`, `pc debug extract --all`, … as **real** (7 neutral nouns
  promoted) → **promotion-precision 0.588**. Its reject-precision is still 1.000 (it never promotes a
  *rejected* noun), so the original bar passed it — yet B would flood the primer with exactly the
  snippet noise finding #1 told us to eliminate. **promotion-precision** generalizes reject-precision
  to *all* non-real classes and is the gate that disqualifies B.
- **A and C promote zero noise** (promotion-precision 1.000): they only ever prime genuinely real
  nouns. A's coarse buckets (Real/Provisional/Suppressed) refuse to promote a single-reference noun at
  all (needs ≥ +3), so single neutral/operate mentions stay Provisional — safe by construction.

## Why A wins on determinism (and C loses)

`reject` is bulletproof, but T-0 showed the `operate_on ↔ neutral` boundary is genuinely fuzzy and
glm is non-deterministic even at temp 0. **A's decision boundary dodges that fuzzy axis**: both
`operate_on (+1)` and `neutral (0)` leave a single-reference noun in the *same* non-promoted
Provisional bucket, so A's verdict is stable (3% flip) against exactly the wobble T-0 flagged. **C
splits Candidate(0) from Provisional(1) right on the fuzzy line**, so a single stance flip flips C's
state → **32% flip** on this single-reference-heavy corpus. B re-reasons holistically each run →
**15% flip**. Determinism is itself high-variance at 2 runs (B was 9%/C 3% on an earlier run), but A
is structurally the steadiest; B and C both breached the 10% bar in the canonical run.

## Recovery (T-A2)

The recovery canary **`episode cards`** (doubted — *"what even are episode cards? did I ask for
those?"* — then adopted and operated on five times) climbs back above threshold under **all three**
scorers: A → +3 **Real**, B → real, C → **Real**. Suppression is never permanent; later ownership
outweighs an early reject. The brief's `fabric-provider` reject is correctly **Suppressed** by A (−6),
**rejected** by B, **Rejected** by C in every run.

---

## Winner & recommendation

**Approach A — signed-delta ledger.** It is the only scorer that clears *every* gate: separation (AUC
1.000, beats freq by 0.50), reject-precision 1.000, **promotion-precision 1.000** (primes zero
noise), recovery ✅, determinism 3% (≤ 10%) — at the **lowest cost** (one shared batched stance pass,
22 calls/run, ~21k tokens; no per-noun calls). It is cheapest, safest, and most stable, and its
robustness is structural, not lucky.

- **Ship A** as the surfacing gate between `build_registry_from_disk` and `detect_first_mentions`:
  only nouns with `signed ≥ +3` prime; `signed ≤ −2` are suppressed.
- **Keep C as a free diagnostic layer** over A's identical events (Dormant/Rejected/Real labels are
  useful for the wiki-doctor/staleness work) — but do **not** gate priming on C's fine-grained states
  on a thin, single-reference corpus; its flip rate is too high there.
- **Keep B as the earned fallback** only if A fails to calibrate on a future, alias-normalized
  population (B has the best real-recall, 0.833, but pays per-noun cost and over-primes noise).

## Honest caveats / nulls

1. **Corpus is single-reference-dominated.** cfv6 yields ~27 entity nouns, almost all referenced in
   one turn, because the **same noun fragments across phrasings** ("context injection" vs "the inject
   hook") and rarely repeats verbatim. The realness ledger's cross-session accumulation therefore
   barely fires on *mined* nouns; the multi-reference signal (and thus recovery / threshold crossing)
   is carried by the canaries. **Alias normalization is the highest-value next step** — without it, A
   keeps almost every real noun at Provisional (real-recall 0.333), not because the scorer is wrong but
   because the surface-string population is fragmented. Recall will rise sharply once aliases are
   merged into one noun_id.
2. **AUC saturates at 1.000** with only 4 rejected nouns and a clean stance axis — it does not
   discriminate A/B/C. The decision rests on promotion-precision, determinism, and cost, which do.
3. **The flip metric is high-variance at 2 runs** (glm temp-0 nondeterminism). A's low flip is the
   robust signal (structural); B's and C's exceedance should be read as "≥ bar / unstable", not a
   precise rate.
4. **gold and judge share a model** (glm-5.1) per the $0-Ollama constraint; the hand-seeded canaries
   (incl. the recovery and dormant cases) are the independent ground-truth anchor, as in T-0.

## Artifacts (committed, reproducible)

- Scorers: `src/realness.rs` (`score_ledger`, `run_lifecycle`/`apply_dormancy`, `judge_holistic`,
  `stance_delta`, cost meter, parse-repair) — 28 unit tests.
- Harness: `src/eval_realness.rs` (`is_entity_candidate`, miner, gold schema + canaries, AUC,
  per-bar metrics) — wired as `pc eval --realness`.
- Frozen gold: `docs/product-spec/realness-artifacts/gold_nouns.jsonl` (+ `population.{jsonl,txt}`,
  `canaries.jsonl`).
- Machine results: `docs/product-spec/realness-artifacts/realness_results.{md,json}`.

```bash
# Re-score against the FROZEN gold (no re-mining; $0; ~12 min on glm cloud):
pc eval --project ~/src/proactive-context \
        --experiment-dir ~/.proactive-context/experiments/cfv6-20260611-012408 --realness
# Re-mine the population for gold curation:
PC_REALNESS_MINE=1 pc eval ... --realness
```

Knobs: `PC_REALNESS_MODEL`, `PC_REALNESS_RUNS` (2), `PC_REALNESS_FREQ_MIN` (3),
`PC_REALNESS_PER_NOUN` (8), `PC_REALNESS_REPAIR_ROUNDS` (2), `PC_REALNESS_BATCH_CHUNK` via
`PC_T0_BATCH_CHUNK` (6).
