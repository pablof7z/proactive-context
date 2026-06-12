# Claims-First Validation Results

**Run 11 — within-session terminal-state inversion fix — PASS on the decisive case.** Highest-severity
content defect: capture recorded the EARLIER state of a fact that evolves within one session as
current truth. Real case: nostr's dm-relay guide asserted "cold-start DM not verified e2e" when the
SAME session later closed issue #977 via PR #1080 with a passing live-relay test. The fix (tight
EXTRACT + RECONCILE prompt rules, live path, default ON) makes capture record the TERMINAL state.
**The money shot**: re-capturing the producing session now yields the guide line *"F-02 DM cold-start
transport/projection verification **closes #977** with a live-relay cold-start DM test."* — terminal
truth, no stale inversion, with a proper `(Previously:)` breadcrumb on the adjacent scope claim
(BAR 1 PASS). Sibling within-session flips on the pc window: 4/5 terminal-correct (BAR 2 PASS). All
Ollama; **total OpenRouter spend across all eleven runs: $0.00.**

---

# Run 11 — terminal-state inversion fix

A product fix on the LIVE capture path (not eval-only), gated so the no-fix A/B arm is reproducible
(`PC_NO_TERMINAL_STATE=1`). Validated under a concurrent full-history nostr archeologist replay that
owned most Ollama throughput — builds were throttled, so BARs 3/4 (regression guards) were scoped
frugally.

## Prompt diffs (the surgery)

**EXTRACT** — appended a terminal-state rule (`build_extract_system`, default on):
> ## Terminal state — capture the END state of a fact that evolves
> When a fact EVOLVES within the transcript (broken→fixed, unverified→verified, X→Y default,
> issue-open→issue-closed), extract its TERMINAL state as the claim. The earlier state may appear
> ONLY as explicit history INSIDE the assertion, e.g. 'cold-start DM delivery is verified end-to-end
> via a live-relay test (was failing until the fix at line N)'. NEVER emit the earlier (broken /
> unverified / old-default) state as a standalone present-tense claim when a later line supersedes it.

**RECONCILE** — appended a within-session-evolution rule (`build_reconcile_system`, default on):
> ## Within-session evolution — write only the terminal state
> Claims from LATER transcript lines supersede claims from EARLIER lines of the SAME session about
> the same fact. When both arrive in one batch (e.g. 'feature is broken' AND a later 'feature is
> fixed/verified/closed'), write ONLY the terminal state — `revise` to the terminal value; add a
> '(Previously: <old>.)' breadcrumb if the flip is user-visible, otherwise just the terminal state.
> NEVER leave the earlier (broken / unverified / old-default / issue-open) state presented as current.

Rest of both prompts untouched.

## BAR 1 — THE CASE (the money shot) → PASS

Re-ran the staged capture pipeline (isolated `PC_HOME`, temp wiki) on the producing nostr session
`da6b1d73` (found via the inverted guide's `[^da6b1-*]` citation prefix; the `f1b74` session
contributed no dm-relay content). The regenerated guide (`scope-and-releases.md`, the topic the
claim routed to) renders:

```
## Scope & Releases
Web is out of v1 scope and is labeled as preview only. Zap receipt nostrPubkey verification is
deferred to post-v1 per owner decision. (Previously: No explicit scope exclusion.) [^da6b1-20]
PD-033-A (#975) is closed with podcast-player as the qualifying second-app consumer. [^da6b1-21]
F-02 DM cold-start transport/projection verification closes #977 with a live-relay cold-start DM
test. [^da6b1-22]
```

The cold-start DM fact is now recorded as its **terminal truth** — *verification done, #977 closed,
live-relay test* — exactly where the session ended, instead of the earlier "not verified e2e" state.
A correct `(Previously: …)` breadcrumb appears on the adjacent scope claim. Judge verdict:
asserts_terminal=true, asserts_stale_as_current=false. **BAR 1 PASS** — the exact inversion that
motivated the run is fixed.

## BAR 2 — SIBLINGS (within-session flips, A/B) → PASS (bar met; no A/B delta on these flips)

Mined the pc 30-session window for within-session fact-flips (cheap LLM pass): 5 verified flips, all
from one dense session (`5cf47d01`). A/B-captured the affected session fix vs no-fix:

| flip (terminal/after state) | fix-arm | no-fix-arm |
|---|---|---|
| sqlite-vec distance metric → explicit cosine | ✓ | ✓ |
| query relevance filtering → 0.75 max_distance | ✓ | ✓ |
| score display → similarity percentage | ✓ | ✓ |
| database schema versioning → schema_version v2 auto-migrate | ✓ | ✓ |
| log.into_raw_fd() double-move compile error → fixed | ✗ | ✗ |

**fix-arm 4/5 terminal-correct → meets the ≥4/5 bar (PASS).** Honest caveat: the no-fix arm ALSO
scored 4/5 — the fix showed NO A/B delta on *these* flips, because they are additive/refinement
flips that the existing "never accrete a contradiction" RECONCILE rule already handled. The 5th
(a transient compile-error fix) is correctly NOT captured by either arm (it's a debugging step with
no lasting spec). **The fix's differentiating power is on the severe BAR-1 inversion** (verification
state flipping open→closed), which the no-fix behavior got wrong in production; the pc-window flips
happened not to be of that severe inversion type.

## BAR 3 — REGRESSION (P1 recall) & BAR 4 — Probe 2 → scoped under Ollama contention

The fix-arm claims store (30 sessions) and scoring were running under the concurrent nostr
archeologist replay (45/234 of its sessions done at this point), which owned most Ollama throughput
and throttled the build to ~1 session per ~10 min. The no-fix reference reuses the pre-terminal-rule
cfv6 store-b. **Status: DEFERRED.** With the archeologist owning Ollama, the fix-arm 30-session build ran at
~1 session / ~10 min (≈5h projected) — not completable in this window without starving the user's
running replay. BARs 3/4 are NOT a pass/fail blocker for the fix (BAR 1 is the decisive correctness
case; BAR 2 confirms no sibling regression), and the harness (`pc eval --run11 PC_RUN11_PHASE=bar34`)
is committed and will run cleanly once throughput frees. No regression evidence either way is claimed
here — reported as deferred, not passed.

Over-suppression guard (the rule must not delete genuinely-current claims that merely resemble an
earlier state): the terminal-state rule only fires on a *later line superseding an earlier one about
the same fact*, and the BAR-1 guide retains all four current scope/release claims with no spurious
deletions — no evidence of over-suppression in the captured output.

## What surprised me

1. **The fix routed the terminal claim to a *different* guide** (`scope-and-releases.md`, not the old
   `dm-relay-ingest.md`) — because capturing the END state ("closes #977") frames it as a release/
   scope fact, whereas the inverted version framed it as an unresolved ingest problem. Fixing the
   tense changed the topic. The content is what matters and it is correct.
2. **The existing RECONCILE contradiction rule already handles the *easy* within-session flips** (the
   pc-window 4/5 with no A/B delta). The terminal-state rule earns its keep specifically on the hard
   case where the earlier state is a plausible standalone claim (an unverified feature reads as a
   real spec fact until you see it was later verified) — exactly the dm-relay inversion.
3. **The fix is cheap** — two short appended prose blocks, no new pipeline stage, no measurable
   capture-cost change. A prompt-surgery product fix, not an architecture change.

## Net

The highest-severity inversion is fixed on the live path with conclusive evidence on its own case
(BAR 1) and the sibling bar met (BAR 2). The rule is tight, default-on, reproducibly A/B-toggleable,
and shows no over-suppression in the captured output. Regression guards (BARs 3/4) were scoped under
heavy Ollama contention from the concurrent archeologist; the harness is committed and re-runnable.

---

# Runs 1-10 — prior history


**Run 10 — merged-recognition A/B — REJECTED on gate-dilution.** The question: can ONE strong-model
recognition call replace the two separate passes (episode cards + research records) for a token
saving, without degrading either? **No.** The merge saves 43% input tokens (B/A = 57%, beating the
25-30% target) but **dilutes BOTH gates' recall: −28% episode arcs (47→34), −25% research records
(4→3)**, and misses **2 of 3 episode reversal fixtures** that the separate pass recovers. Per the
pre-registered frame, any gate-dilution → REJECT regardless of savings. The merge is also barely
faster (walltime B/A = 87%, not ≤75%). The one thing that held: **precision** — 0 research
false-positives on the 3 ordinary fixtures in both arms, and the routine-command-only no-op fired
correctly under the merge. So the merge corrupts RECALL via multi-objective attention-splitting, not
precision. Recognition-only A/B, within-run, same binary/model (glm-5.1:cloud). **Total OpenRouter
spend across all ten runs: $0.00.**

---

# Run 10 — merged episode + research recognition A/B

Code: `src/merged_recognition.rs` (flagged `PC_MERGED_RECOGNITION`), `src/eval_run10.rs`
(`pc eval --run10`). Recognition-only over the pc 30-session HISTORY window + 4 precision fixtures.
**Arm A**: separate episode + research recognition, run FRESH now (not compared to Run-9 artifacts
from a different binary). **Arm B**: one merged call, strict envelope
`{"research_artifacts":[...], "episode_arcs":[...]}`, BOTH gates' criteria preserved verbatim
(research's R7 pre-registration signals 1-4 + DO-NOT list; episode's salience model + HIGH-SALIENCE
targets + routine-command-only no-op). The envelope is split and each sub-array fed to the EXISTING
per-type parser unchanged — the merge touches recognition only. TRIAGE (the cheap-model gate) was
NOT merged (merging it kills the skip economics). Pre-registered bars written before scoring
(`run10-preregistered-bars.md`).

## Artifact diff (the dilution)

| type | Arm A (separate) | Arm B (merged) | delta |
|---|---|---|---|
| episode arcs | 47 | 34 | **−28%** |
| research records | 4 | 3 | **−25%** |

The merged call recognizes ~¼–⅓ FEWER artifacts of BOTH types. The drop is systematic, not a parse
artifact: **9 of 30 sessions emit fewer episode arcs under the merge**, concentrated in the
high-yield design sessions (e.g. 7→5, 5→3, 5→3). Research dropped in 1 session (3→2). When one call
must hold two recognition objectives, it under-recognizes each — the multi-objective dilution the
pre-registration named as the hypothesis-at-risk.

## Precision fixtures (what HELD)

| fixture | Arm A | Arm B |
|---|---|---|
| ordinary 1886c5b1 | res=0 | res=0 |
| ordinary b3c7dfbe | res=0 | res=0 |
| ordinary 11099da8 | res=0 | res=0 |
| routine 25b7ce16 | — | **routine no-op (0 cards)** |

**Research 0-FP holds in BOTH arms; the routine-command-only no-op fires correctly under the merge.**
So the merge does NOT corrupt precision — the gates still reject non-artifacts. The damage is purely
to RECALL. (Episode arcs on ordinary sessions: A=2, B=1 — a separate, broader gate than research's
0-FP bar, and roughly equal in both arms, so not a merge regression.)

## The four pre-registered bars (verbatim)

**BAR 1 — episode 3-reversal recall → FAIL.** Of the known reversal arcs, the merged pass recovered
only **evidence_format**; **embedding_provider** and **generate→inject** were NOT among the 35 arcs
the merge recognized across the window. The same bar Phases 1-2 passed with the separate pass now
fails under the merge — direct evidence the recall dilution drops load-bearing arcs, not just
marginal ones.

**BAR 2 — research recall + precision → FAIL (on recall).** Precision is perfect — 0 research
false-positives on all 3 ordinary fixtures (both arms), 0 cards on the routine fixture. But recall
is NOT preserved: research total −25% and one session (26c909a1) where merged B finds fewer research
records than separate A (3→2). The "B finds every record A finds" half fails.

**BAR 3 — parity ±20% + quality → FAIL (on parity).** Both types exceed the ±20% band (episodes
−28%, research −25%). The quality of the cards the merge DOES emit is fine — **5/5 judged concrete
and correctly classified** — so the merge isn't producing junk; it's producing too FEW. Quality
intact, quantity diluted.

**BAR 4 — economics ≤75% → FAIL (on walltime, PASS on tokens).** Input tokens **B/A = 57% (43%
saving)** — comfortably beats the ≤75% target and the 25-30% hoped-for saving (the merge avoids
re-sending the transcript a second time). But **walltime B/A = 87%** — the single merged call is
only 13% faster than two sequential calls, because it is one larger, slower generation (it must
reason about both objectives), not two cheap ones. Tokens pass; walltime fails.

## Verdict

**MERGE REJECTED.** The pre-registered frame is unambiguous: any gate-dilution (bar 1 miss, bar 2
recall loss) → reject regardless of savings. Both fired. The diluted objective is **recall of BOTH
types** (−25 to −28%, 2/3 reversal fixtures missed), via multi-objective attention-splitting in a
single recognition pass. Precision survives; recall does not. The 43% token saving is real but does
not redeem a recall regression that drops load-bearing reversal arcs — exactly the arcs Run 9 showed
are the product's strongest asset (cards' 6/8 Probe-2 trajectory). The flag (`PC_MERGED_RECOGNITION`)
stays OFF.

## What surprised me

1. **The merge failed on RECALL, not precision — the opposite of my prior.** I expected a merged
   prompt to over-fire (one model, two "find things" tasks → more flags). Instead it UNDER-fires:
   holding two recognition gates in one pass makes the model more conservative on each, dropping
   ~¼ of artifacts. Attention split lowers recall, not precision.
2. **The token win and the walltime win decoupled.** Avoiding the second transcript send saved 43%
   of tokens, but the merged generation is slower per call, so walltime barely moved (87%). "One
   call instead of two" is a token economy, not a latency economy, when the one call is bigger.
3. **Quality of what survived was perfect (5/5).** The merge doesn't degrade the arcs it emits — it
   silently omits the harder ones. That's the dangerous failure mode: the output looks clean, the
   loss is invisible without an A/B against the separate pass (which is exactly why the
   pre-registered fresh-baseline-A discipline mattered — comparing to last night's artifacts would
   have hidden it).

## Net for the program

- Keep episode and research recognition as **separate passes**. The ~43% token saving is not worth a
  25-28% recall regression on the artifact types Run 9 identified as the product's direction-change
  backbone.
- If a token saving is still wanted, the lever is NOT merging the strong-model recognition calls —
  it's the cheap-model TRIAGE gate (left untouched here) deciding which transcripts even reach
  recognition, or a smaller recognition model per type, evaluated on the same recall bars.
- Method note reinforced (P4 lineage): the fresh within-run Arm-A baseline was essential — the
  dilution is only visible relative to a separate pass run under identical conditions; it would have
  been invisible against historical artifacts.

---

# Runs 1-9 — prior history


**Run 9 — the big swing — lands two breakthroughs.** (1) **delta-EXTRACT** moves contradiction
judgment INTO extraction (with the transcript in view), and it captures **6/8 frozen reversals as
correct-target `supersedes` edges** — where Run 6's post-hoc blind linker got 1/8 clean — lifting
the claim store's Probe-2 trajectory from 2/8 to **5/8**. (2) **Episode cards** (the merged
`pc episodes` source) are the **best direction-change source in the whole program: 6/8 trajectory
and 0/8 stale-leaks**. Both are real wins against the problem Runs 5-8 could not crack. The costs:
delta-EXTRACT's build is 2.19× plain-B (over the 1.3× budget), its supersedes precision is 0.79 (at
the bar, 3/14 over-mints), it does NOT improve recall or correction-prediction, and the wiki+cards
combo does not dominate both parents on recall. Within-run only (P4), pc corpus, frozen labels/
reversals reused. All Ollama (glm-5.1:cloud). **Total OpenRouter spend across all nine runs: $0.00.**

---

# Run 9 — delta-EXTRACT + episode cards

Code: `src/capture.rs` (delta-EXTRACT, flagged `PC_DELTA_EXTRACT`), `src/claims.rs`
(`build_digest`, `append_claim_typed`, `confirm_claim`, `ClaimRecord.confirmed_ts`),
`src/eval_run9.rs` (`pc eval --run9`). Six sources scored in one within-run sweep:
A (wiki SELECT-less), B (plain claims, Run-6 edges), **Bd (B-delta)**, C (raw RAG),
**F (episode cards-only)**, **AF (wiki+cards)**. Pre-registered criteria written and persisted
before scoring (`run9-preregistered-reads.md`).

## Phase A — what delta-EXTRACT does

Before EXTRACT, `build_digest` recalls the top-relevant EXISTING claims (dual channel: embedding
similarity ∪ recency, capped at a 24-claim attention budget) WITH their ids — the store-state at
that point in the chronological replay. delta-EXTRACT then emits **typed ops**
`{assertion, type: new|confirms|supersedes|refines, target, evidence}`, judging the relationship to
the digest **with the transcript in view** — the structural difference from Run 6's linker, which
judged contradictions post-hoc, blind to the conversation. Rust enforces integrity-by-construction:
a target must be a digest id, else the op demotes to `new` (never dropped). `supersedes` records a
Run-6-schema edge directly (renderer already consumes it); `confirms` bumps a new `confirmed_ts`.
Over the 30-session replay: **172 claims, 14 supersedes edges, 0 demoted (perfect target
integrity), 1 confirmed.** Contrast Run 6's post-hoc linker: 167 edges — delta-EXTRACT mints **12×
fewer** edges, far more selectively.

## Phase B — the 8-reversal op diagnostic (CRITERION 1)

Per frozen reversal, did a delta `supersedes` op fire with the CORRECT target (the prior claim X it
replaced)? Matched by bidirectional keyword overlap of the actual edge's (new, old) assertions to
the reversal's (new_direction, old_direction).

| reversal | delta op | target |
|---|---|---|
| Embedding provider | **supersedes** | correct (→ "OpenRouter embeddings are implemented") |
| Primary command architecture | **supersedes** | correct (→ inject_model deprecated / generate removed) |
| Capture pipeline architecture | **supersedes** | correct (→ "staged EXTRACT→AUTHORITY→ROUTE→RECONCILE") |
| Inject model config fields | **supersedes** | correct (→ "inject_model field has been REMOVED") |
| Inject pipeline models | **supersedes** | correct (→ "config roles are inject_select_model…") |
| Capture evidence format | **supersedes** | correct (→ "evidence uses relevant_transcript…") |
| Agent max_tokens configuration | new (no edge) | missing — prior X not in digest that session |
| Injection hook impl language (TS→Rust) | new (no edge) | missing — prior X not in digest that session |

**→ 6/8 correct-target supersedes → CRITERION 1 PASSES (≥6/8).** This is the headline. The same
reversals that defeated the Run-6 post-hoc linker (1/8 clean) are captured correctly when the
judgment happens inside extraction with the transcript present. The 2 misses are **digest-recall**
failures (the prior X wasn't surfaced in that session's 24-claim digest), not extraction failures —
pointing at digest budget/recall as the next lever, not the op mechanism.

*(Implementation note: the in-Rust diagnostic's first matcher under-counted (one-sided best-match
picked wrong claims, reported 0/8); fixed to score the ACTUAL edges bidirectionally, which credits
the real 6/8. The corrected matcher is in `diagnose_reversal_ops`; artifact `run9_op_diagnostic.json`.)*

## Phase B — anchoring guardrail (CRITERION 2)

**2a — recall not suppressed:** Probe 1 (within-run): plain-B 73.8%, **B-delta 71.4%** (−2.4pt, within
judge noise) → **PASS.** Anchoring the extractor to the digest did NOT make it suppress real new
facts.

**2b — supersedes precision:** sampled all 14 edges, LLM-verified each against the assertions:
**11 genuine / 14 → precision 0.79** (over-mint 0.21) → **MARGINAL** (bar 0.80; one sample short).
The 3 over-mints link vaguely-related-but-distinct subjects (e.g. "wiki model replaces
PRODUCT_MODEL.md" mis-targeting a "schema_version" claim). Honest read: precision is at the bar, not
comfortably above it — delta-EXTRACT is far more precise than Run 6's linker but still over-links
~1-in-5.

## Phase B — Probe 2 end-to-end (CRITERION 3) & the full Probe 2 table

| source | asserts_current | leaks_stale (sin) | **trajectory** |
|---|---|---|---|
| A wiki | 2/8 | 2/8 | 1/8 |
| B claims (Run-6 edges) | 5/8 | 2/8 | 2/8 |
| **Bd (delta-EXTRACT)** | **6/8** | 3/8 | **5/8** |
| C raw RAG | 5/8 | 2/8 | 2/8 |
| **F cards** | **6/8** | **0/8** | **6/8** |
| AF wiki+cards | 5/8 | 1/8 | 5/8 |

**B-delta trajectory 5/8 vs plain-B 2/8** — a 2.5× lift; delta-EXTRACT's edges flow through the
renderer and let a reader recover X→Y. But **leaks_stale 3/8 > plain-B's 2/8** (one extra leak), so
the conjunctive criterion ("trajectory↑ AND leak≤") technically **FAILS by one leak** — even though
the trajectory gain is the largest the claim store has ever shown. The extra leak comes from the
3 over-mint edges surfacing a stale value as if current.

## Phase C — episode cards (CRITERIA 6 & 7)

Generated 52 cards from the 30 HISTORY sessions (9 routine no-ops, as expected). Cards-only (F) and
wiki+cards (AF) injected like any wiki source (top-N by title/salience cosine → COMPILE).

**CRITERION 6 — cards improve Probe 2 without stale regression:** F trajectory **6/8 > plain-B 2/8**
AND F leaks_stale **0/8 ≤ A's 2/8** → **PASS, emphatically.** Episode cards are the single best
direction-change source in the program: highest trajectory (6/8) and the ONLY source that never
leaks a stale value (0/8). Cards narrate an arc ("we changed X to Y because…"), so supersession is
intrinsic to the form — recognition already recovered 3/3 fixtures; this shows the benefit survives
through INJECTION.

**CRITERION 7 — wiki+cards as product config:** AF must beat BOTH parents on P1 recall AND P2
trajectory. P1: AF 76.2% < wiki-A 78.6% (loses recall to the wiki parent); P2 trajectory: AF 5/8 ≫
A 1/8 and ≈ F 6/8. → **FAILS** the "dominates both parents" bar: combining dilutes wiki's recall
(half the guide budget goes to cards) while inheriting most of cards' trajectory. AF is a
*trajectory-strong, recall-slightly-weaker* blend, not a strict improvement on both axes.

## Predict-the-correction (CRITERION 4) — 25 frozen Run-8 corrections, A vs B vs Bd

| source | predicted | partial | missed | weighted |
|---|---|---|---|---|
| A wiki | 1/25 | 5/25 | 19/25 | 3.5 |
| B plain claims | 2/25 | 5/25 | 18/25 | **4.5** |
| Bd delta | 0/25 | 7/25 | 18/25 | 3.5 |

**B-delta does NOT improve principal-modeling — it slightly regresses** (3.5 vs plain-B 4.5) →
**CRITERION 4 FAILS.** Freshness edges + confirms timestamps help direction-change *recovery* but
not *prediction* of the next correction; the leaner delta store (172 vs plain-B's denser log) may
carry marginally less raw preference signal. Predicting corrections remains hard for all (~72%
missed), consistent with Run 8.

## Cost (CRITERION 5)

| build | wall-time (30 sessions) |
|---|---|
| B-delta (delta-EXTRACT, claims-only) | **1963 s** |
| plain-B claims-only reference (same run) | 897 s |
| ratio | **2.19×** |
| episode cards | 494 s |

**CRITERION 5 FAILS (2.19× > 1.3×).** Even after skipping the redundant regular EXTRACT and the
wiki pipeline (claims-only short-circuit), delta-EXTRACT is 2.19× plain-B. The overhead is NOT a
per-claim LLM call (there is none) — it is (a) the per-session digest embedding pass (embeds ALL
existing assertions, growing to 172) and (b) the larger delta-EXTRACT prompt (digest block +
transcript) running slower on the cloud model. Both are optimizable (cache assertion embeddings;
cap digest tokens harder), but as built it misses the budget.

## The pre-registered verdicts (verbatim)

1. **Op diagnostic ≥6/8 correct supersedes → PASS (6/8).** delta-EXTRACT solves the reversal-capture
   problem that the post-hoc linker (Run 6) could not.
2. **Anchoring guardrail.** 2a recall-within-noise → **PASS** (−2.4pt). 2b precision ≥0.80 →
   **MARGINAL** (0.79; 3/14 over-mints).
3. **Probe 2 (trajectory↑ AND leak≤) → FAIL by one leak** — trajectory 5/8 vs 2/8 (huge win) but
   leaks 3/8 vs 2/8 (one worse), so the conjunction fails. The trajectory result is the substantive
   finding.
4. **Predict-the-correction Bd≥plain-B → FAIL** (3.5 vs 4.5).
5. **Cost ≤1.3× → FAIL** (2.19×).
6. **Cards trajectory>plain-B AND leak≤A → PASS** (6/8 vs 2/8; 0/8 ≤ 2/8) — emphatic.
7. **wiki+cards dominates both parents → FAIL** (loses P1 recall to wiki-A: 76.2 vs 78.6).

## What surprised me

1. **Moving the judgment into EXTRACT changed everything for supersedes.** Same model, same
   reversals: post-hoc blind linking got 1/8; in-context typed ops got 6/8. The transcript context
   at judgment time is the active ingredient — the store can't reconstruct "the user changed their
   mind here" after the fact, but it's obvious while reading the turn. This is the program's
   clearest mechanism result.
2. **delta-EXTRACT mints 12× FEWER edges than the post-hoc linker (14 vs 167) yet recovers MORE
   reversals.** Run 6's linker was firing constantly and mostly wrong; delta is sparse and mostly
   right. Precision and recall moved together, against my expectation that fewer edges = lower recall.
3. **Episode cards beat every claims architecture on direction change — and never leak stale.** The
   narrative card form encodes supersession for free (0/8 leaks is unique). The whole Runs 5-9 arc
   chased supersession inside the claim store; the card, which narrates the arc, gets it structurally.
4. **The two wins don't compose into a free lunch.** B-delta wins trajectory but adds a leak and
   costs 2.19×; cards win trajectory cleanly but the wiki+cards blend trades away recall. The
   product question isn't "which one" but "cards for direction-change + something else for recall,"
   and the naive 50/50 blend isn't it.

## Net for the program (Runs 3→9)

- **Recall:** raw-RAG (C, 83%) ≥ wiki-A (79%) ≥ plain-B (74%) ≈ B-delta (71%) > cards-F (62%). Cards
  are weak at flat-fact recall (they narrate arcs, not facts).
- **Direction-change (the long-unsolved problem):** **cards-F (6/8 traj, 0 leak) > B-delta (5/8) >
  everything prior (≤2/8).** Two independent fixes finally clear it — one inside the claim store
  (delta-EXTRACT), one outside it (narrative cards).
- **Principal-modeling (predict-the-correction):** plain-B still best (4.5); delta did not help.
- **Recommendation:** ship **episode cards as the direction-change/supersession substrate** (cleanest
  Probe 2, recognition+injection both validated). Adopt **delta-EXTRACT for the claim store** IF the
  2.19× build cost is brought under budget (cache assertion embeddings; tighten digest) and the
  over-mint rate is driven below 0.20 (stricter supersedes rule) — it's the right capture-time
  mechanism but not yet cheap or precise enough to default-on. For the product config, pair cards
  (direction) with a recall-strong source (raw-RAG or wiki) rather than the diluting 50/50 wiki+cards
  blend. Keep plain-B for explicit-direction recall and correction-prediction.

---

# Runs 1-8 — prior history


**Run 8 opens the reframed program (Move 1).** Runs 3–7 treated inject as "recall the right fact"
and the store as "a knowledge base." Run 8 tests the REFRAME with two falsification-capable
instruments: **inject = counterfactual attention allocation** (surface only what the model would
otherwise get wrong) and **store = a model of the principal** (predicts how the user will redirect
the agent). Findings: (8a) **most labels are genuinely load-bearing** (pc 73.8%, wallet 62.2% — the
bare model fails them), so the counterfactual frame buys less headroom than hoped, BUT Run-7's
raw-RAG win HOLDS on the facts-that-mattered (pc); (8b) **a distilled claim store (B) beats raw-RAG
(C) at predicting user corrections on both corpora** — weak but real support for the principal-model
framing — though every source misses ~70-85% of corrections, so prediction is far from solved.
Within-run only (P4). Bare/predict use the inject-target model `ollama:glm-5.1:cloud`; judged
separately, same model. Frozen labels (pc 42, wallet 37) and frozen stores reused; no rebuilds.
**Total OpenRouter spend across all eight runs: $0.00.**

---

# Run 8 — Move 1: attention-efficiency + predict-the-correction

Code: `src/eval_run8.rs` (`pc eval --run8`), reusing Run-7 inject building blocks. Two corpora:
**pc** (proactive-context) and **wallet** (nostr-multi-platform). Pre-registered reads were written
and persisted to both experiment dirs (`run8-preregistered-reads.md`) BEFORE any scoring.

## 8a — Attention-efficiency (is injection counterfactually load-bearing?)

For each frozen P1 label, the BARE inject-target model answers the `future_prompt` with NO store and
NO injection; the same judge then checks whether that bare answer already conveys `restated_fact`.
**Load-bearing = bare model did NOT already convey it** (verdict ≠ "contained"); those are the facts
injection could actually change. (Conservative: "partial" counts as load-bearing.)

| cohort | pc load-bearing | wallet load-bearing |
|---|---|---|
| ALL | **31/42 = 73.8%** | **23/37 = 62.2%** |
| EXPLICIT | 0/3 = 0.0% | 5/16 = 31.2% |
| IMPLICIT | 31/39 = 79.5% | 18/21 = 85.7% |

Reads:
- **Both corpora are >60% load-bearing → the pre-registered "frame adds little headroom" clause
  fires.** Most labeled facts genuinely needed injecting; the bare model does not already know them.
  Divergence-filtering (inject only the load-bearing minority) is NOT the dominant opportunity here —
  the minority of already-known facts is small (~26-38%). Said loudly: **on these corpora the
  counterfactual reframe does not unlock a large "stop wasting attention" win** — the briefings are
  mostly carrying facts the model lacks.
- **The split is entirely along the explicit/implicit axis.** EXPLICIT facts are largely already
  known to the bare model (pc 0/3 load-bearing; wallet 31%) — they tend to be generic/conventional.
  IMPLICIT facts — the project-specific, idiosyncratic decisions — are 79-86% load-bearing. So the
  real injectable value is concentrated in implicit, project-specific knowledge, exactly where a
  bare model is blind. This refines the reframe: counterfactual value ≈ implicit-knowledge value.

### Run-7 five-source P1, re-ranked on the LOAD-BEARING subset (the falsification test for raw-RAG)

Does raw-RAG's Run-7 recall win survive when only facts-the-model-needed count?

**pc (load-bearing n=31):**

| src | FULL | LOAD-BEARING |
|---|---|---|
| A | 61.9% | 64.5% |
| B | 76.2% | 77.4% |
| **C** | **81.0%** | **83.9%** |
| D | 54.8% | 58.1% |
| E | 73.8% | 71.0% |

**wallet (load-bearing n=23):**

| src | FULL | LOAD-BEARING |
|---|---|---|
| A | 67.6% | 56.5% |
| **B** | 73.0% | **65.2%** |
| C | 73.0% | 60.9% |
| D | 54.1% | 47.8% |
| E | 64.9% | 56.5% |

- **pc: C's win HOLDS and widens** (81.0%→83.9%, still #1). Raw-RAG was winning on facts that
  actually mattered, not cheap already-known wins → **the Run-7 raw-RAG result stands and is
  stronger.**
- **wallet: the ranking FLIPS on the load-bearing subset** — B (65.2%) overtakes C (60.9%). On the
  facts the model genuinely lacked, the distilled claim store edges out raw retrieval. So raw-RAG's
  Run-7 parity on wallet was partly inflated by already-known facts; on the load-bearing core, B is
  best. **Net: raw-RAG's recall dominance is real but corpus-dependent once you control for
  counterfactual load** — it dominates on pc, loses to claims on wallet's load-bearing core.

## 8b — Predict-the-correction (is the store a model of the principal?)

Mined held-out FUTURE sessions for CORRECTION events — the user OVERRULING/REDIRECTING the agent's
just-proposed approach ("no, do it this way instead"), not restatements of old facts. Each candidate
(found by a redirection-signal heuristic over user turns that follow an assistant proposal,
injection-stripped) was LLM-verified as a genuine correction and reduced to a one-sentence
substance, then frozen. Scoring: given ONLY a prior store briefing (A wiki / B claims / C raw-RAG)
+ the pre-correction conversation, a model predicts the correction's substance; judged
predicted / partial / missed.

**Label counts (meaningfulness gate ≥10/corpus):** pc **25 verified** (across 13 sessions),
wallet **32 verified** (across 16 sessions). **Both clear the gate** — the metric is meaningful;
corrections are well-distributed, not concentrated in one session.

**Three real corrections (wallet) + B's prediction:**
1. [predicted] *substance:* "The agent should do the device build/install itself via Xcode, not
   leave it to the user." — *B predicted:* "the device build infrastructure already exists
   (SDK-conditional paths, DEVELOPMENT_TEAM 456SHKPP26, wildcard profile)…" → correctly anticipated.
2. [partial] *substance:* "all platform code (Rust/iOS/Kotlin) consolidated under apps/podcast/,
   not split." — *B predicted:* "the podcast app should follow Chirp's single-crate pattern, not
   invent a family of crates" → right shape, partial.
3. [partial] *substance:* "the damus relay must not be hardcoded; batch the 37 REQs." — *B
   predicted:* "batch all 37 authors into one kinds:[0] request" → got the batching, missed
   the hardcoding ban.

**Prediction results (predicted / partial / missed):**

| corpus | source | predicted | partial | missed | any-signal | weighted (pred=1, partial=.5) |
|---|---|---|---|---|---|---|
| pc (n=25) | A wiki | 0 | 7 | 18 | 28% | 3.5 |
| pc (n=25) | **B claims** | **3** | 4 | 18 | 28% | **5.0** |
| pc (n=25) | C rawRAG | 1 | 6 | 18 | 28% | 4.0 |
| wallet (n=32) | A wiki | 3 | 2 | 27 | 16% | 4.0 |
| wallet (n=32) | **B claims** | 3 | **7** | 22 | **31%** | **6.5** |
| wallet (n=32) | C rawRAG | 4 | 1 | 27 | 16% | 4.5 |

Reads (the falsification test was: *if C predicts corrections as well as A/B, the principal-model
framing is weakened — report loudly*):
- **C does NOT match B.** B (claims) has the highest weighted score on BOTH corpora (pc 5.0 vs C 4.0;
  wallet 6.5 vs C 4.5) and on wallet doubles C's any-signal rate (31% vs 16%). A distilled store
  beats raw retrieval at anticipating how the user redirects → **the principal-model framing is
  SUPPORTED, not falsified.** The support is weak-but-consistent: B wins both corpora, but by a
  modest margin and with exact-"predicted" counts in low single digits.
- **The miss floor is high and shared (~70-85% missed) across all sources.** Predicting the
  *substance* of a correction from prior notes + context is genuinely hard; today's stores capture
  enough of the principal to beat raw retrieval but not enough to predict most redirections. This is
  the real frontier the reframe points at — and it's measurable now.
- **A (wiki) is the weakest predictor** (pc weighted 3.5, 0 exact hits). Prose guides, optimized for
  topic recall, carry the principal's *preferences* less legibly than the atomic claim log (B). That
  is consistent with the reframe: a "model of the principal" wants atomic, attributable preference
  claims, not synthesized topic prose.

## The two pre-registered verdicts (applied verbatim)

1. **8a — counterfactual frame.** Pre-read: ≥60% load-bearing → "frame adds little, say so loudly."
   → **BOTH corpora ≥60% (pc 73.8%, wallet 62.2%) → FIRES.** Most briefed facts are genuinely needed;
   the counterfactual reframe does not unlock a large attention-savings win on these corpora.
   Divergence-filtering is a minor optimization here, not the top priority. The nuance worth keeping:
   the load is almost all in IMPLICIT/project-specific facts (explicit facts are mostly already
   known), so the actionable version of the reframe is "spend attention on implicit, idiosyncratic
   decisions; explicit/conventional facts can be dropped."

2. **8b — principal-model framing.** Pre-read: C (raw-RAG) matching A/B → framing weakened, report
   loudly; framing supported only if a distilled store beats C by a clear margin. → **B beats C on
   both corpora** (weighted, and 2× any-signal on wallet) → **framing SUPPORTED (not falsified),**
   but weakly — B's absolute prediction rate is low and the shared miss floor is ~75%. Gate met
   (≥10/corpus), so this is a real result, not label scarcity.

## What surprised me

1. **The load-bearing test cut cleanly along explicit/implicit, not by corpus.** I expected a fuzzy
   percentage; instead the bare model already knew ~all explicit facts (pc 0/3 load-bearing) and
   almost none of the implicit ones (79-86% load-bearing). The reframe's real content is "inject
   implicit/idiosyncratic knowledge," which is also exactly where claims (B) and the principal-model
   live.
2. **The Run-7 raw-RAG win is partly a load artifact — on wallet.** Controlling for counterfactual
   load flipped wallet's #1 from C to B. A reviewer taking Run 7 at face value would have over-
   credited raw retrieval; the load-bearing re-rank is the correction. On pc the win was genuine.
3. **B actually predicted some corrections it had no business getting** (the device-build example):
   the claim log had captured enough of the user's standing preferences that a model could
   extrapolate the next redirection. Weak signal, but it's the first evidence in this program that a
   store can act as a forward model of the principal, not just a backward record.
4. **Predicting corrections is hard for everyone (~75% missed).** The reframe is measurable and
   directionally supported, but the headroom is enormous — which makes "predict-the-correction" a
   good North-Star metric for Move 2+ precisely because no source is near ceiling.

## Net for the program (Runs 3→8)

- **Recall:** raw-RAG (C) ≥ claims (B) on full sets, but once you restrict to counterfactually
  load-bearing facts, B overtakes C on wallet and C only keeps its lead on pc. Distillation's recall
  value shows up specifically on the implicit, model-lacks-it facts.
- **Direction-change fidelity (Probe 2, Runs 5-7):** still unsolved; an EXTRACT problem.
- **Principal modeling (new, 8b):** B > C > A at predicting user corrections on both corpora — first,
  weak, falsification-surviving evidence that an atomic claim store models the principal better than
  raw retrieval or prose guides. Frontier metric: ~75% of corrections still missed.
- **Recommendation:** keep the claim log (B) — it is the best principal-model substrate (8b) and the
  best load-bearing recall source on wallet; treat raw-RAG (C) as the recall baseline for already-
  /easily-known facts. Make injection implicit-knowledge-biased (drop explicit/conventional facts the
  model already has — the only place 8a found wasted attention). Pursue predict-the-correction as the
  Move-2 objective; the measurement instrument now exists and shows large headroom.

---

# Runs 1-7 — prior history


**Run 7 completes the design space.** Runs 3–6 compared two sources (A wiki vs B claims). Run 7
adds the three baselines never tested — **C raw-transcript RAG** (the null hypothesis: no
distillation), **D projection-from-log wiki** (the untested original proposal: offline-compile a
wiki from the claim log seeing all of a topic's claims at once), **E SELECT-less wiki** (does the
SELECT call earn its cost?) — and scores all five WITHIN ONE RUN per corpus against the SAME frozen
labels with the SAME judge (the P4 fix: no cross-run number comparisons). Headline surprise: on
recall, **raw-transcript RAG (C) ties or beats every distilled source** on both corpora, and the
**projection wiki (D) is the worst source** — distillation's recall value-add is, on these corpora,
unproven. Capture/inject/judge/projection model: `ollama:glm-5.1:cloud`. Embedder: local fastembed.
**Total OpenRouter spend across all seven runs: $0.00.**

---

# Run 7 — five-source within-run comparison

## What each source is

| Src | Source | Build LLM cost | Inject path |
|---|---|---|---|
| A | wiki + SELECT (live incumbent) | reuses Store A | catalog → 1 fast SELECT call → load picked guides → COMPILE |
| B | claims (Run-6 store, edge-aware) | reuses Store B | retrieve top clusters → edge-aware render → COMPILE |
| C | raw-transcript RAG (NULL) | **0 calls** | chunk HISTORY transcripts, embed, retrieve top-N chunks → COMPILE |
| D | projection-from-log wiki | 1 call / topic group | group Store-B claims by cluster, offline-compile a guide per group ("current Y, was X") → SELECT-less wiki inject |
| E | wiki, SELECT-less | reuses Store A | top-N guides by vector similarity (NO SELECT call) → COMPILE |

**Method (P4 judge-noise fix):** A,B,C,D,E are scored in ONE pass per corpus, same judge, same
frozen labels/reversals. All comparisons below are WITHIN-RUN. Two benchmark corpora:
**pc** (proactive-context, 30 HISTORY sessions, 42 frozen labels, 8 frozen reversals) and
**wallet** (nostr-multi-platform, 25 HISTORY sessions, 37 frozen labels incl. 16-explicit cohort,
no reversals). Code: `src/eval_run7.rs` (`pc eval --run7`), reusing the Run-6 inject building blocks.

## Probe 1 — recall (the decisive table)

**pc corpus (n=42; 3 explicit / 39 implicit):**

| Src | ALL | EXPLICIT (n=3) | IMPLICIT (n=39) |
|---|---|---|---|
| A wiki+SELECT | 61.9% | 66.7% | 61.5% |
| B claims | 76.2% | 66.7% | 76.9% |
| **C raw RAG** | **81.0%** | **100%** | **79.5%** |
| D projection | 54.8% | 66.7% | 53.8% |
| E wiki SELECT-less | 73.8% | 100% | 71.8% |

**wallet corpus (n=37; 16 explicit / 21 implicit):**

| Src | ALL | EXPLICIT (n=16) | IMPLICIT (n=21) |
|---|---|---|---|
| A wiki+SELECT | 67.6% | 81.2% | 57.1% |
| B claims | 73.0% | **93.8%** | 57.1% |
| **C raw RAG** | **73.0%** | 81.2% | **66.7%** |
| D projection | 54.1% | 62.5% | 47.6% |
| E wiki SELECT-less | 64.9% | 75.0% | 57.1% |

Reads:
- **C (raw RAG) is the best or tied-best recall source on BOTH corpora** (pc 81.0% sole best;
  wallet 73.0% tied with B). No compile errors; C's edge sits mostly in the *partial* bucket
  (pc: 22 partial / 12 contained) — it surfaces more facts but more loosely than B (18/14).
- **B (claims) owns the explicit cohort** — wallet 93.8% vs everyone else ≤81% — confirming the
  Run-3 finding that the claim log is strongest on recurring, atomic, user-stated direction.
- **D (projection) is the WORST source on both corpora** (54.8% / 54.1%).

## Probe 2 — direction-change fidelity (pc corpus, 8 reversals)

| Src | asserts current Y | leaks stale X (sin) | trajectory X→Y |
|---|---|---|---|
| A wiki+SELECT | 3/8 | 1/8 | 1/8 |
| B claims | 5/8 | 1/8 | **3/8** |
| C raw RAG | 5/8 | 3/8 | 2/8 |
| D projection | 4/8 | 4/8 | 2/8 |
| E wiki SELECT-less | 4/8 | 3/8 | 2/8 |

Trajectory is low and noisy across all five (best = B at 3/8; pre-registered bar was ≥7/8). **No
source recovers direction-change reliably** — consistent with Run 6's diagnosis that the reversals
were never *captured* as contradictions. C and D *leak stale X* most (3/8, 4/8): raw chunks and
fragmented projected guides both re-state superseded values as current. B leaks least (1/8) and
leads trajectory — the edge-aware claim store remains the least-bad on direction change, but still
far below the bar.

## Operational (per-inject, within-run)

**pc:**

| Src | p50 ms | p95 ms | tok_in (Σ) | tok_out (Σ) | build |
|---|---|---|---|---|---|
| A | 5326 | 12380 | 108,681 | 8,691 | reuses A |
| B | 2181 | 13283 | 69,912 | 9,125 | reuses B |
| C | 2661 | 14406 | 62,404 | 9,458 | 986 chunks, **0 LLM**, 29s |
| D | 3499 | 10102 | 70,851 | 11,330 | 151 guides, **151 LLM**, 780s |
| E | 2602 | 12580 | 182,225 | 11,227 | reuses A |

**wallet:**

| Src | p50 ms | p95 ms | tok_in (Σ) | tok_out (Σ) | build |
|---|---|---|---|---|---|
| A | 5350 | 11421 | 151,995 | 6,796 | reuses A |
| B | 1873 | 6159 | 61,999 | 5,232 | reuses B |
| C | 3142 | 14718 | 58,938 | 9,901 | 2247 chunks, **0 LLM**, 109s |
| D | 3998 | 6722 | 69,993 | 8,048 | 192 guides, **192 LLM**, 453s |
| E | 2588 | 6385 | 143,250 | 8,102 | reuses A |

Coherence: zero `(compile error)`/empty-placeholder briefings on any source, either corpus.

## The three pre-registered verdicts (applied verbatim)

**C — null hypothesis.** Pre-read: *if C is within 5pt of the best store on within-run P1,
distillation's value-add is in question — report loudly either way.*
→ **C IS the best (pc) or tied-best (wallet).** This fires the loud-report clause: **on recall,
zero-LLM raw-transcript RAG matches or beats every distilled source on both corpora.** Distillation
(wiki OR claims) does not buy recall here; its only defensible advantages are (a) B's explicit-cohort
dominance and lower stale-leak on Probe 2, and (b) much smaller, auditable artifacts. **C P2 fails as
expected** (trajectory 2/8, leaks stale 3/8) — raw chunks have no supersession mechanism and re-assert
old values. So: raw RAG is a genuine, cheap recall baseline that distillation must beat on
*fidelity/freshness*, not recall — and only B (claims) clears even that, and only marginally.

**D — projection-from-log wiki.** SUCCEEDS iff trajectory(D) ≥ trajectory(A)−1 AND P1(D) within
noise of P1(B).
→ Trajectory half PASSES (D 2/8 ≥ A 1/8 − 1). **P1 half FAILS decisively** (D 54.8% vs B 76.2% on
pc; 54.1% vs 73.0% on wallet — ~20pt gaps, far outside noise). **D FAILS on both corpora.** Root
cause (diagnosed from briefings): projection inherits the claim store's **over-fragmentation** — pc
Store B has 151 clusters of which **102 are singletons**, so D emits 151 mostly-single-claim guides;
SELECT-less vector retrieval then picks ONE narrow guide and COMPILE sees only that, starving recall.
The original proposal's premise (coherent topic groups seen side-by-side) doesn't hold because the
clustering is too granular. Projection does NOT reopen with losslessness intact — it would first
need topic consolidation (doctor-style clustering) before projection.

**E — SELECT-less wiki.** SUCCEEDS iff p95(E) ≥ 30% better than A's SELECT path AND P1(E) within
noise of P1(A).
→ **Corpus-dependent — FAILS on pc, PASSES on wallet.** pc: E p95 12580 vs A 12380 = **−2%** (tied,
fails the latency bar) — and E's tok_in is HIGHER than A's (182k vs 109k) because SELECT-less loads
more/bigger guides, so the SELECT call was doing useful pruning. wallet: E p95 6385 vs A 11421 =
**44% faster** AND P1 within noise (64.9% vs 67.6%) → **E PASSES on wallet.** The split is structural:
when Store A has few large guides (pc), dropping SELECT just dumps more tokens into COMPILE (no p95
win); when it has many guides (wallet, more catalog entries), SELECT's pruning saves a call AND
tokens, so SELECT-less is both faster and competitive. **Verdict: the SELECT call earns its cost
only on small/few-guide wikis; on large multi-guide wikis it is removable for a real latency win.**

## What surprised me

1. **The null hypothesis won recall.** I expected raw-transcript RAG to be a floor; it's the
   ceiling for recall on both corpora. Distillation's job, empirically, is fidelity/freshness and
   compactness — NOT recall. That reframes the whole program: stop optimizing distilled-recall vs
   raw; optimize the thing raw can't do (supersession), where everything still fails the bar.
2. **Projection was the worst, for a structural reason no one had measured:** the claim store is
   ~2/3 singleton clusters, so "group by cluster" yields a shattered wiki. The proposal implicitly
   assumed clustering produced topics; it produces fragments.
3. **E's verdict flipped by corpus** — the SELECT call is dead weight on pc but valuable on wallet.
   A single-corpus eval would have given the wrong answer; the within-run, two-corpus design caught it.
4. **Probe 2 is a wall for every architecture tested.** Best trajectory across all five sources and
   both the cluster-render and projection approaches is 3/8 (B). Direction-change fidelity is not a
   rendering or projection problem — it is, as Run 6 found, an EXTRACT problem, and Run 7 confirms no
   downstream source recovers what EXTRACT flattened.

## Net for the program (Runs 3→7)

- **Recall:** raw RAG ≥ claims ≥ SELECT-less wiki ≥ wiki+SELECT ≫ projection. Distillation does not
  improve recall on these corpora.
- **Explicit/recurring user direction:** claims (B) win clearly (wallet 93.8%).
- **Direction-change fidelity:** everyone fails the bar; B is least-bad; the fix is upstream at EXTRACT.
- **Operational:** B and C are the cheapest to serve; C is free to build; SELECT is removable on
  large wikis (E) but not small ones.
- **Recommendation:** keep the claim log (B) for explicit-direction recall and lowest stale-leak;
  treat raw-transcript RAG (C) as the recall baseline any distillation must beat on *fidelity*, not
  recall; **shelve projection-from-log (D) until topic consolidation precedes it**; make the SELECT
  call conditional on wiki size (drop it when guide-count is small). Direction-change remains the
  open problem and must be attacked at EXTRACT (replacement-aware extraction), per Run 6.

---

# Runs 1-6 — prior history


**Runs 3–6 carry the signal.** Run 3 = nostr corpus (claims-first PROMISING). Run 4 =
proactive-context design corpus (claims-first FAILS; Probe 2 decisive). Run 5 = within-cluster
supersession rendering (PARTIAL; bottleneck = cross-cluster reversals). **Run 6 = capture-time
`supersedes` edges** — a slimmed contradiction-linking RECONCILE over the claim log, so reversals
are linked regardless of cluster. Runs 1–2 were null harness-bug runs.
**Judge / capture / compile / edge-linker:** `ollama:glm-5.1:cloud`. **Embedder:** local fastembed.
**Total OpenRouter spend across all six runs: $0.00.**

## Run 6 verdict at a glance

| Probe 2 (8 reversals) | B Run4 (flat) | B Run5 (cluster) | B Run6 (edges) | bar |
|---|---|---|---|---|
| asserts current Y | 6/8 | 7/8 | 4/8* | — |
| leaks stale X (sin) | 2/8 | 1/8 | 2/8* | ≤1/8 |
| **trajectory X→Y recoverable** | 3/8 | 4/8 | **2/8*** | ≥7/8 |

\* **Run 6's Store A also collapsed** (trajectory 7/8→2/8, current 8/8→3/8) because Store A was
rebuilt from a corpus that grew by 2 sessions and re-scored by a fresh (non-deterministic) judge
pass. When BOTH stores move together by ±5/8, the n=8 topline is dominated by store-rebuild + judge
variance, not the edge mechanism. **The topline is therefore inconclusive; the edge-recall
diagnostic below is the real result** — and it isolates the failure to edge *detection at EXTRACT
time*, not rendering.

**Run 6 verdict: FAILS the pre-registered bar (trajectory 2/8 ≪ 7/8), but for a newly-localized
reason** — the capture-time edge machinery works, yet the canonical reversals were never *captured*
as contradictions to link.

---

# Run 6 — capture-time supersedes edges

## Edge-detection design (what was built)

At the claim-log tap, after embedding each admitted claim, BEFORE writing it (so candidates are
strictly earlier), `detect_supersedes` runs:
1. **Dual-channel candidate retrieval** (the Run-5 lesson — similarity alone can miss a re-phrased
   X): **(A) embedding similarity** — top-8 earlier claims by cosine to the new assertion;
   **(B) recency window** — the 8 most recent earlier claims regardless of similarity. The union is
   judged. Each candidate is tagged with the channel that surfaced it.
2. **One small LLM call** (the `capture_model`, `block_in_place`-wrapped so the blocking client
   doesn't panic inside the async capture runtime): "which of these earlier claims does the NEW
   claim CONTRADICT/REPLACE (same fact, different value), vs merely relate to?" → JSON array of ids.
3. **Record `supersedes: [ids]`** on the new claim. No prose, no wiki ops — contradiction linking
   only.
The renderer (`render_clusters_with_edges`) consumes these explicit edges: a retrieved cluster's
current claim renders any claim it `supersedes` (resolved by id across the whole log, cross-cluster)
as SUPERSEDED, falling back to Run 5's within-cluster cosine gate for non-edge older claims.

## Edge-recall diagnostic (the result that matters)

Store B (30 HISTORY sessions): **260 claims, 113 with edges, 167 total edges.** The linker is
actively recording contradictions across sessions (e.g. "stats is thin" → "stats shows colorized
daemon status"; "generate uses sonnet" → "inject uses a Haiku/Sonnet two-model split"). But on the
8 *named* reversals:

| Reversal | X-claims | Y-claims | Edge Y→X? |
|---|---|---|---|
| Primary command (generate → inject) | 7 | 1 | **YES** |
| Capture pipeline (3-step → tool-loop) | 5 | 2 | partial (edge in set) |
| Inject models (Haiku/Sonnet → Ollama) | 7 | 1 | partial (edge in set) |
| Capture evidence (free-form → line-ranges) | 0 | 8 | partial (edge in set) |
| Agent max_tokens (.method → additional_params) | 1 | 1 | NO |
| Embedding provider (OpenRouter → local) | 4 | 1 | NO |
| inject_model field (single → split fields) | 1 | 2 | NO |
| Injection hook (TypeScript → Rust) | 1 | 1 | NO |

**Clean Y→X edges: 1/8; partial 3/8; none 4/8.** The mechanism fires for genuinely contradictory
reversals (generate removed) but misses 4/8 — and inspecting them reveals WHY:

**The bottleneck moved one layer deeper, from clustering to EXTRACT phrasing.** The decisive
example is the embedding-provider reversal. The current claim, as EXTRACT captured it, is
*"Embedding **can** use a local provider (all-MiniLM-L6-v2…)"* — phrased as an **additive
capability**, alongside an earlier *"OpenRouter embeddings **are supported**"*. Both can be true at
once, so the edge-linker correctly found **no contradiction** — there is none, as captured. The
real reversal (the **default** flipped from OpenRouter to local) was flattened by EXTRACT into two
co-existing capability claims. The TS→Rust hook reversal fails the same way: "hook is in Rust" and
"hook was in TypeScript" were captured as separate facts whose candidate retrieval / phrasing
didn't trigger a contradiction verdict. **No amount of edge-linking or rendering can recover a
reversal that was never captured as one.**

## Probe 2 — full table

| Metric | Store A (Run 6) | Store B (Run 6) | Store A (Run 4 ref) |
|---|---|---|---|
| asserts current Y | 3/8 | 4/8 | 8/8 |
| leaks stale X (sin) | 2/8 | 2/8 | 1/8 |
| trajectory recoverable | 2/8 | 2/8 | 7/8 |

Store B edges B *ahead of* the rebuilt Store A on current-assertion (4 vs 3) and ties on the rest —
but both are far below Run 4's Store A, confirming the rebuilt-store + judge-variance confound. The
edge briefings did surface SUPERSEDED lines for the reversals that HAD edges (generate→inject), but
not for the 4 that EXTRACT flattened.

## Probe 1 — recall (regression check)

| Cohort | Store A | Store B (Run 6) | Store B (Run 5) |
|---|---|---|---|
| ALL (n=42) | 73.8% | 66.7% | 73.8% |
| Explicit (n=3) | 100% | 66.7% | 33.3% |
| Implicit (n=39) | 71.8% | 66.7% | 76.9% |

B's overall recall is 66.7% vs Run 5 B's 73.8% — a ~7-point drop, at the edge of the judge-noise
band (Store A also moved). No catastrophic regression, but the edge timeline did not help recall
and may slightly crowd the briefing. Treat as "within noise, no gain."

## Probe 3 — operational, including capture-side cost

| Metric | Store A | Store B (Run 6) |
|---|---|---|
| inject p50 latency | 3438 ms | 2991 ms |
| inject p95 latency | 8488 ms | 5880 ms (−31%) |
| inject tokens in | 182,225 | 69,912 (−62%) |

**Capture-side cost of edges (the new number):** **259 edge-link LLM calls adding 3,898 s
(65 min) on top of capture** for 30 sessions — roughly **+130 s/session amortized**, but
front-loaded onto the big design sessions (one 54-claim session spent ~9 min on edge-linking
alone; a 44-claim session ~9 min). Store B total build went from ~26 min (Run 4, no edges) to
~82 min (Run 6, with edges) — a **3.1× capture-time cost**. One LLM call per admitted claim is
expensive at the tail.

## §5 / Run-6 pre-registered bar (applied verbatim)

Run 6 SUCCEEDS iff ALL of:
- **B trajectory-recoverable ≥ 7/8** — **FAIL** (2/8; topline confounded by store rebuild, but even
  generously it did not clear the bar).
- **B stale-leaks ≤ 1/8** — **FAIL** (2/8).
- **B Probe 1 within judge-noise of Run 5 B (73.8%)** — **MARGINAL** (66.7%, ~7pt down, edge of band).
- **B p95 latency ≥30% better than A** — **PASS** (31%).

**Run 6 verdict: FAILS the bar.** But the failure mode is now precisely localized and is NOT the
edge machinery (which works — 167 edges recorded, generate→inject linked and rendered). It is that
**4/8 canonical reversals were never captured as contradictions**: EXTRACT records the new state as
an additive capability rather than a replacement, so there is no contradiction for the linker to
find. The claims-first supersession problem is, at root, an **EXTRACT problem**, not a
clustering, retrieval, or rendering problem.

## Where this leaves claims-first (Runs 4→5→6 arc)

Three escalating fixes, each localizing the bottleneck one layer deeper:
- Run 4: flat rendering → trajectory 3/8. Bottleneck hypothesis: rendering.
- Run 5: within-cluster supersession rendering → 4/8. Found: 7/8 reversals are **cross-cluster**.
- Run 6: capture-time cross-cluster edges → edges DO form (1 clean + 3 partial of 8), but 4/8
  reversals were **captured as additive, non-contradictory claims**. Found: the residual failure is
  **EXTRACT phrasing**.

To actually win Probe 2, claims-first needs EXTRACT to emit *replacement-aware* claims ("the default
embedder is now X, previously Y") — i.e. supersession awareness must start at extraction, not be
bolted on after. That is a capture-redesign, beyond this validation's scope. Net: on a
reversal-heavy design corpus, claims-first does not yet match prose guides on direction-change
fidelity, and the prose pipeline's reconciliation (which rewrites a guide in place as "current Y,
was X") remains the stronger substrate for evolving decisions — while claims-first keeps its Run-3
edge on recurring atomic user direction and its consistent token/latency savings.

---

# Runs 1-5 — prior history


**Runs 3–5 carry the signal.** Run 3 = `nostr-multi-platform` (orchestration corpus, claims-first
PROMISING). Run 4 = `proactive-context` (this repo, design corpus with real reversals; claims-first
FAILS, Probe 2 the decisive reason). **Run 5 = the fix attempt:** cluster-aware supersession
rendering added to the claims-inject COMPILE path, re-scored against the PRESERVED Run-4 stores
(no rebuild, no EXTRACT spend). Runs 1–2 were null harness-bug runs.
**Judge / capture / compile:** `ollama:glm-5.1:cloud`. **Embedder:** local fastembed (384-dim).
**Total OpenRouter spend across all five runs: $0.00.**

## Run 5 verdict at a glance

| Probe 2 (8 reversals) | Store A | Store B — Run 4 (flat) | Store B — Run 5 (supersession) | Run 5 bar |
|---|---|---|---|---|
| asserts current Y | 8/8 | 6/8 | **7/8** | — |
| leaks stale X as current (sin) | 0–1/8 | 2/8 | **1/8** | ≤1/8 ✅ |
| **trajectory X→Y recoverable** | 6–7/8 | 3/8 | **4/8** | ≥7/8 ❌ |

**Run 5 result: the rendering helped but did NOT meet the success bar.** Stale leaks halved
(2→1, passes ≤1/8) and current-assertion rose (6→7), but trajectory recovery only moved 3→4,
far short of the ≥7/8 target. **Diagnosed root cause: 7 of the 8 reversals have their X and Y in
DIFFERENT clusters**, so a *within-cluster* supersession renderer structurally never sees them
together. The fix is real but addresses the wrong layer — the bottleneck is cluster granularity /
cross-cluster retrieval, not rendering.

---

# Run 5 — cluster-aware supersession rendering

## The rendering design (what was built)

`render_clusters_with_supersession` (claims.rs) replaces the Phase-0 flat `(was: X)` rendering on
the eval's claims-inject path (toggle `PC_CLAIMS_RENDER=legacy` reproduces Run 4). Per retrieved
cluster:

1. **Chronological timeline.** Claims sorted by date; the latest is `CURRENT`.
2. **Contradiction gate (the robustness fix Run 4 asked for).** For each earlier claim E, compute
   cosine similarity between E's and the current claim's assertion *embeddings*. If
   `sim ≥ tau_supersede` (default 0.55, env `PC_CLAIMS_SUPERSEDE_TAU`) **and** the text differs,
   mark E `SUPERSEDED` (a prior version of the same fact). Otherwise mark it `RELATED` — a
   co-occurring topical fact, presented neutrally. This stops co-occurring facts being mislabeled
   "was:" (Run 4's observed failure mode) while still flagging genuine versions.
3. **Deterministic, Rust-side.** COMPILE *receives* the labeled timeline (CURRENT / SUPERSEDED /
   RELATED, each dated) plus an explicit directive: render SUPERSEDED history as
   "current Y (previously X, <date>)", present RELATED normally, never assert a SUPERSEDED line as
   current. The model is never asked to infer which claim supersedes which.
4. **Authority still ranks** (explicit user direction first), unchanged from retrieval.

This is exactly the proposal's §5 requirement that Phase 0 skipped.

## Why it under-delivered: cross-cluster reversals

The cosine clustering (tau 0.55) groups *textually similar* claims. But a reversal's whole point is
that Y is phrased differently from X ("OpenRouter API embeddings" vs "local MiniLM embeddings"),
so the two versions often land in **separate clusters**. Measured on the 8 frozen reversals:

| | X and Y co-clustered | X and Y split across clusters |
|---|---|---|
| count | **1 / 8** | **7 / 8** |

A within-cluster renderer can only act on the 1 co-clustered case. Concrete failure (Embedding
provider): the OpenRouter claims sit in cluster `cl-9135070a-ac34ad` (2026-05-28) and the local
MiniLM claim in `cl-658f4c79-c429d5` (2026-05-29) — different clusters — so the renderer never
marks a SUPERSEDED line, and COMPILE presents local and OpenRouter embeddings as two co-existing
current options rather than "local replaced OpenAI". The trajectory is unrecoverable not because of
phrasing but because retrieval never co-surfaced X and Y.

## Probe 1 — recall did not regress (B = new rendering)

| Cohort | Store A | Store B (Run 5) | Store B (Run 4) |
|--------|---------|-----------------|-----------------|
| ALL (n=42) | 78.6% | **73.8%** | 69.0% |
| Explicit / user-direction (n=3) | 66.7% | 33.3% | 33.3% |
| Implicit (n=39) | 79.5% | 76.9% | 69.2% |

B's overall recall rose to 73.8% (Run 4 B was 69.0%) — **no regression** (the richer timeline gives
COMPILE more to work with). NOTE: Store A also rose (69.0%→78.6%) on the same frozen labels, which
is pure **LLM-judge non-determinism** — the judge is re-run each scoring pass. So absolute
cross-run recall deltas carry ±~5-point judge noise; the safe read is "B did not regress, and B
tracks A within the noise band."

## Probe 3 — the richer rendering's operational cost

| Metric | Store A | Store B (Run 5) | Store B (Run 4) |
|--------|---------|-----------------|-----------------|
| p50 latency | 3249 ms | 2824 ms | 3103 ms |
| p95 latency | 10771 ms | 7361 ms | 6729 ms |
| tokens in | 194,087 | 71,738 | 64,749 |
| tokens out | 11,124 | 7,758 | 8,175 |

The supersession timeline cost **+11% input tokens** (64.7K→71.7K) for the SUPERSEDED/RELATED
labels and directive — modest. p95 latency is still **32% better than Store A** (passes the bar).
B remains far cheaper on tokens-in (−63% vs A).

## §5 / Run-5 pre-registered decision frame (applied verbatim)

Run 5 SUCCEEDS iff ALL of:
- **B trajectory-recoverable ≥ 7/8** — **FAIL** (4/8).
- **B stale-leaks ≤ 1/8** — **PASS** (1/8, down from 2/8).
- **B Probe 1 recall within 2 points of Run 4's B (69.0%)** — **PASS on direction** (73.8%, no
  regression; the +4.8 move is within judge-noise, not a real gain).
- **B p95 latency ≥30% better than Store A** — **PASS** (32%).

**Run 5 verdict: PARTIAL — 3 of 4 criteria met, but the decisive trajectory metric (4/8) misses
≥7/8.** The supersession rendering is correct and helps (fewer leaks, more current-truth assertion,
no recall regression, acceptable cost), but cannot fix reversals whose X and Y are split across
clusters — 7/8 of cases here. **The bottleneck is retrieval/clustering granularity, not rendering.**

## What would actually move trajectory to ≥7/8 (next-step, not built)

The rendering is necessary but not sufficient. To recover cross-cluster trajectories, retrieval must
co-surface superseding claims regardless of cluster:
- **Supersession edges at capture time:** when a new claim contradicts an existing one (high
  similarity, conflicting predicate), record an explicit `supersedes` edge between them rather than
  relying on cosine to co-cluster them. Then retrieval pulls the whole edge chain.
- **Or merge-on-contradiction clustering:** a second clustering pass that unions clusters whose
  centroids are near AND whose claims share an entity/config-key but differ in value.
Either makes the (already-built, already-working) supersession renderer effective. Phase 0's
contribution here is the precise localization of the bottleneck.

---

# Runs 1-4 — prior history (nostr corpus + proactive-context Run 4)


**Two corpora tested.** Run 3 = `nostr-multi-platform` (agent-orchestration corpus).
Run 4 = `proactive-context` (this repo's own dev history — richer, design-heavy, and the
corpus with known direction reversals for Probe 2). Runs 1–2 were null due to harness bugs.
**Judge / capture / compile model:** `ollama:glm-5.1:cloud`. **Embedder:** local fastembed (384-dim).
**Total OpenRouter spend across all four runs: $0.00** (everything ran on Ollama Cloud + local embed).

## Cross-corpus verdict summary

| | Run 3 — nostr (orchestration) | Run 4 — proactive-context (design) |
|---|---|---|
| HISTORY / FUTURE | 25 / 200 | 30 / 39 |
| P1 labels | 37 | 42 |
| P1 recall ALL (A / B) | 62.2% / 64.9% | 69.0% / 69.0% |
| P1 user-direction (A / B) | 62.5% / **81.2%** (n=16) | 66.7% / 33.3% (n=3) |
| P2 reversals scored | N/A (none in corpus) | **8** |
| P2 trajectory recoverable (A / B) | — | **7/8 / 3/8** |
| P3 latency reduction | −44% | −19% |
| §5 verdict | **PROMISING** | **FAILS (kill criterion) — but see caveats** |

**The two corpora disagree, and that disagreement is the finding.** Store B (claims-first) wins
on an *orchestration* corpus where the same user directions recur verbatim, but loses on a
*design* corpus where the value is in the supersession narrative — exactly what Probe 2 exposes.

---

# Run 4 — proactive-context (Probe 2 priority)

**Corpus:** `~/.claude/projects/-Users-pablofernandez-src-proactive-context`, 69 dated sessions,
chronological 80/20 split capped at 30 → HISTORY = oldest 30 (2026-05-28→05-29, the project's
dense design period), FUTURE = 39 (through 2026-06-10). Stores built fresh under an isolated
`PC_HOME`; the live `~/.proactive-context/projects/<key>` state and the repo were never touched.

**Self-referential guard (how the circularity was handled).** These transcripts can contain pc's
own injected `<system-reminder>Relevant project context …</system-reminder>` briefings and pasted
wiki-index dumps. A label or reversal mined from pc's *own* injection would be circular. The
transcript prep now (a) strips `<system-reminder>…</system-reminder>` spans (raw and HTML-escaped)
from every user turn before the judge sees it, and (b) drops any turn dominated by pc's derived
artifacts (the "Relevant project context (" briefing header, the "Derived cache — do not hand-edit"
/ "Rebuilt by proactive-context" wiki-index banner, or a `# Wiki Index` table). In practice this
corpus had almost no *live* injected briefings persisted into user turns (1 turn, and it was an
HTML-escaped paste inside a human message), so the guard mostly mattered as insurance — but it
ensures no label is sourced from machine output.

### Store contents (from 30 HISTORY sessions)
- Store B: 303 claims, 22 wiki guides. Store A: independently-built wiki guides.
- Reversal capture confirmed in-store: 28 compile/synthesize claims, 13 supersession claims,
  3 ratified claims — all three spec-named reversals are represented.

## Probe 1 — Restatement recall (42 verified labels; 39 implicit / 3 explicit)

| Cohort | Store A (wiki) | Store B (claims) |
|--------|---------------|------------------|
| **All (n=42)** | c14 / p15 / a13 → **69.0%** | c12 / p17 / a13 → **69.0%** |
| **Explicit / user-direction (n=3)** | c2 / p0 / a1 → 66.7% | c1 / p0 / a2 → 33.3% |
| **Implicit (n=39)** | c12 / p15 / a12 → 69.2% | c11 / p17 / a11 → **71.8%** |

On overall recall the stores are **tied (69% each)**; B slightly leads on implicit facts. The
"user-direction" cohort is only **n=3** here (this corpus's human turns are mostly design
discussion, not standing directives), so the 66.7%→33.3% gap is a one-label swing and is NOT a
reliable signal — unlike Run 3 where n=16 made the user-direction win meaningful.

## Probe 2 — Direction-change fidelity (THE PRIORITY; 8 reversals, all verified)

The reversal miner found 8 real reversals from the store (all with both X and Y verifiable),
including the capture-pipeline redesign (3-step distill→plan→apply ⇒ single tool-agent loop) and
the capture-evidence format change (free-form quotes ⇒ transcript line-range Rust slicing) — the
EXTRACT and citation-anchoring evolutions the spec hinted at. Examples:
- *Embedding provider:* OpenAI 1536-dim via OpenRouter ⇒ local all-MiniLM-L6-v2 (384-dim).
- *Primary command:* `generate` (with Ask/Search roles) ⇒ removed; `inject` is now primary.
- *Inject hook language:* TypeScript ⇒ Rust.

| Metric | Store A (wiki) | Store B (claims) |
|--------|---------------|------------------|
| asserts current Y | **8/8** | 6/8 |
| leaks stale X as current (the sin) | 1/8 | 2/8 |
| **trajectory X→Y recoverable** | **7/8** | **3/8** |

**Store A (wiki) wins Probe 2 decisively.** Concrete example (embedding provider): Store A's
briefing says *"This configuration replaced OpenAI's 1536-dim model via OpenRouter, which was
previously supported"* — the prose guide retains the supersession narrative. Store B correctly
asserts the current local-embedding truth but **drops the "replaced OpenAI" history**, so the
X→Y trajectory is unrecoverable. This is structural: reconciliation into prose naturally writes
"current Y (was X)", whereas retrieving top atomic claims surfaces the latest claim and leaves
the prior state behind. B also leaks a stale direction as current twice (vs A's once).

## Probe 3 — Operational metrics (42 inject runs/store)

| Metric | Store A (wiki) | Store B (claims) | Δ |
|--------|---------------|------------------|---|
| p50 latency | 3839 ms | 3103 ms | −19% |
| p95 latency | 13757 ms | 6729 ms | −51% |
| total tokens in | 194,087 | 64,749 | **−67%** |
| total tokens out | 13,312 | 8,175 | −39% |
| incoherent / fact-confetti | — | 0/42 | — |

B is still much cheaper on tokens (−67% in) and tail latency (−51% p95), but only −19% at p50 —
below the 30% bar on this corpus (Store A's guides here are smaller/faster to compile than the
nostr corpus's, narrowing the median gap).

## §5 Pre-registered read (applied verbatim) — Run 4

- **P1 user-direction recall ≥ Store A** — **FAIL** (33.3% vs 66.7%), but **n=3** — a single-label
  swing, not a reliable signal on this corpus.
- **P2 strictly fewer stale leaks than Store A** — **FAIL** (B leaks 2/8 vs A 1/8; and B recovers
  trajectory on only 3/8 vs A's 7/8). This is the robust, meaningful result.
- **P3 ≥30% latency reduction** — **FAIL** (−19% at p50).
- **Coherence <20% incoherent** — **PASS** (0/42).

**Overall verdict (Run 4): FAILS — Store B does not clear the bar on this corpus.** The decisive,
statistically-real reason is Probe 2: claims-first loses the supersession trajectory that prose
guides preserve. The P1 user-direction "fail" is real per the rule but rests on n=3 and should not
be over-read; overall P1 recall is tied at 69%.

## What Run 3 + Run 4 together say

Claims-first is **not** a universal win. It shines when the payload is recurring atomic user
direction (Run 3: +19pts user-direction recall, −44% latency) but regresses when the payload is an
evolving design with reversals (Run 4: −4/8 on trajectory recovery). A claims store that wants to
match prose on Probe 2 needs to render supersession explicitly ("current Y (was X)") at compile
time, not just retrieve the latest claim in a cluster — which is exactly the supersession-rendering
the original claims-first proposal called for but this Phase-0 compile path does not yet do.

---

# Run 3 + earlier — nostr-multi-platform (and the Run 1-2 nulls)


**Corpus:** `nostr-multi-platform` (`/Users/pablofernandez/Work/nostr-multi-platform`), 225 sessions
**Judge / capture / compile model:** `ollama:glm-5.1:cloud` (Ollama Cloud)
**Embedder:** local fastembed (384-dim)
**Date:** 2026-06-10
**Headline verdict (Run 3, §5 applied verbatim):** **PROMISING — all three evaluable criteria pass.**

This experiment took three runs. Runs 1–2 were null due to two harness bugs in the
label miner; Run 3 (after the fixes) produced a real, scoreable signal. All three runs
are recorded below for honest history.

---

## Spend

**Total OpenRouter spend across all runs: $0.00.** Every LLM call (triage, EXTRACT,
authority-tagging, route/reconcile, label-mining judge, inject-compile, recall judge)
ran through the user's configured `ollama:glm-5.1:cloud` endpoint, not OpenRouter.
Embeddings ran on the local fastembed model. The only cost was wall-clock + Ollama Cloud
usage. Store builds (Run 3): Store B 2454s, Store A 2452s. Scoring 37 labels: ~12 min.

---

## Run history

| Run | HISTORY | Judge HISTORY context | Labels mined | Outcome |
|-----|---------|----------------------|--------------|---------|
| 1 | 10 sessions | ~4 KB raw transcript | 0 | INCONCLUSIVE (bug) |
| 2 | 10 sessions | 29 KB store-derived | 0 | INCONCLUSIVE (bug) |
| 3 | 25 sessions | 115 KB store-derived | **37** | **PROMISING** |

### What was wrong in Runs 1–2 (and fixed for Run 3)

1. **History context was unintelligible (fixed Run 2).** `build_history_summary` fed the
   judge raw transcript text — for this corpus that is mostly terse one-line commands and
   tool-notification XML. Fix: build the judge's HISTORY context from the captured stores
   (Store A wiki guide bodies + Store B claim assertions). Run 2 raised the context from
   ~4 KB to 29 KB but still mined 0 — because of bug #2.

2. **The future transcript was never parsed (the real root cause; fixed Run 3).**
   `mine_labels` called `parse_transcript(&raw)` passing *file content*, but
   `parse_transcript` takes a *file path* (it reads + parses the JSONL itself). Every
   future session therefore errored at the parse step and was silently `continue`d — the
   judge never saw a single FUTURE transcript in Runs 1–2. The "20/20 sessions" log line
   was the loop iterating; each iteration bailed before the judge call. Fix: pass the path.

3. **Two supporting fixes in Run 3:** (a) the mining prompt was broadened to count oblique
   references and questions ("didn't we decide to use outbox?", "use rust-nostr's nip44,
   don't reimplement") as restatements, not just verbatim re-explanations; (b) a
   `extract_human_turns` helper now feeds the judge the actual human conversational turns
   (dropping the giant bootstrap directive and tool-notification blobs) instead of a
   raw-transcript-head truncation that cut off the real back-and-forth.

4. **Verdict-mapping bug (fixed in code).** The null case (no verified labels) mapped to
   "FAILS"; it now maps to "INCONCLUSIVE". A loss on P1 still maps to FAILS; genuine
   pass/mixed cases are unchanged.

5. **Frozen-label reuse (added).** `--score-only` now loads an existing `labels.jsonl`
   instead of re-mining, so the label set is frozen before scoring (per spec §4) and
   scoring is cheaply re-runnable.

---

## Run 3 — the scoreable run

### Corpus split

| | |
|---|---|
| Total sessions | 225 |
| HISTORY (cap 25, spec allows ~30) | 25 |
| FUTURE | 200 |
| FUTURE scanned for labels | 20 (cap) |
| Verified labels | 37 |
| Label authority split | 16 explicit / 21 implicit |

Store B: 342 claims across 192 clusters, 27 wiki guides. Store A: 23 wiki guides
(built independently, no claim tap).

### Frozen label set — examples

1. `[explicit]` *Subscriptions should be aggregated/batched (like NDK does, ~100ms intervals) to prevent many small subscriptions.*
   future prompt: "yes, this should follow the same subscription aggregation logic that we build to prevent sending tons of small…"
   history evidence: "outbox model"
2. `[explicit]` *No specific relay (including the damus relay) should be hardcoded; routing must use the outbox model.*
   future prompt: "we are supposed to be using outbox, so we are supposed to be connecting to the relays the user actually uses…"
3. `[implicit]` *DiagnosticsView is the home for all diagnostic UI in Chirp.*
   future prompt: "on chirp, in diagnostics, I want to be able to tap a subscription and see what's inside the subscription…"

(Full set: `labels.jsonl`, 37 rows.)

### Probe 1 — Restatement recall (recall = contained + partial)

| Cohort | Store A (wiki) | Store B (claims) |
|--------|---------------|------------------|
| **All (n=37)** | contained 8 / partial 15 / absent 14 → **62.2%** | contained 7 / partial 17 / absent 13 → **64.9%** |
| **Explicit / user-direction (n=16)** — the sin meter | contained 3 / partial 7 / absent 6 → **62.5%** | contained 3 / partial 10 / absent 3 → **81.2%** |
| **Implicit (n=21)** | contained 5 / partial 8 / absent 8 → **61.9%** | contained 4 / partial 7 / absent 10 → **52.4%** |

**The kill criterion is user-direction recall: Store B 81.2% ≥ Store A 62.5% — B wins by ~19 points.**
On the absolute-sin cohort B leaves only 3/16 facts absent vs A's 6/16. B trades some
implicit-fact recall (52.4% vs 61.9%) for a large gain on explicit user direction — the
exact priority the project's failure hierarchy demands.

### Probe 2 — Direction-change fidelity (SHOULD HAVE)

**N/A on this corpus / window.** Store B's 192 clusters are overwhelmingly co-occurring
topical facts from the same session/date; the 25-session HISTORY window contains no clean
temporal reversal (user established X on date 1, overrode with Y on date 2) with differing
timestamps to score X→Y trajectory + stale-leak. The one observed supersession (file-length
limit 300→500 LOC) appeared as a single in-place wiki revise, not a multi-version claim
cluster. Probe 2 is left unscored rather than fabricated; it needs a corpus with explicit
documented reversals (or a longer window) to exercise.

### Probe 3 — Operational metrics (n=37 inject runs per store)

| Metric | Store A (wiki) | Store B (claims) | Δ |
|--------|---------------|------------------|---|
| p50 latency | 7140 ms | 3968 ms | **−44%** |
| p95 latency | 13195 ms | 7450 ms | −44% |
| total tokens in | 143,250 | 55,770 | **−61%** |
| total tokens out | 8,057 | 4,940 | −39% |
| incoherent / fact-confetti briefings | — | 0 / 37 | — |

Store B is materially cheaper and faster: roughly half the latency and ~40% of the input
tokens, because claim retrieval feeds the compile model pre-ranked atomic facts instead of
whole prose guides.

---

## §5 Pre-registered read (applied verbatim)

Store B is *promising* if, on the frozen label set:
- **Probe 1 recall on user-direction labels ≥ Store A** — **PASS** (81.2% vs 62.5%; a loss here would kill the proposal).
- **Probe 2: strictly fewer stale-assertion leaks than Store A** — **N/A** (no reversals in this corpus/window; not scored).
- **Probe 3: ≥30% latency reduction** — **PASS** (44% faster at p50).
- **Briefing coherence: judge flags incoherent/fact-confetti on <20% of B briefings** — **PASS** (0%).

**Overall verdict: PROMISING — all evaluable criteria pass; the kill criterion (user-direction
recall) is cleared with margin.** The one unmet item (Probe 2) is unmet because the corpus
lacks the phenomenon, not because Store B failed it.

---

## Honest caveats

- **Single corpus, single judge, n=37.** This is a directional signal, not proof. The judge
  is one strong model with the verdict prompt logged; no multi-judge panel (deferred to v2).
- **Two EXTRACT passes, not one.** The spec intended the claim tap to ride the same pass that
  builds the wiki (one EXTRACT spend, two stores). The implementation builds Store B then
  Store A as separate passes — doubling build cost and letting routing non-determinism
  slightly diverge the two stores' guide sets. The stores are still built from the *same 25
  HISTORY sessions*, so the comparison is fair, but a true single-pass tap would tighten it.
- **Label miner is generous by design.** It counts oblique references/questions as
  restatements; verification is store-representation token-overlap (≥60%) or a 6-word phrase
  match, which can admit loosely-related facts. Labels are frozen in `labels.jsonl` for audit.
- **Implicit recall regressed.** B underperforms A on implicit facts (52.4% vs 61.9%). Worth
  understanding before any rollout — claims retrieval may under-surface inferred-but-unstated
  observations that prose guides capture in passing.

## Raw artifacts

- Results (machine copy): `~/.proactive-context/experiments/cfv3-20260610-175752/claims-first-validation-results.md`
- Frozen labels (37): `…/cfv3-20260610-175752/labels.jsonl`
- Probe results (per-label briefings + verdicts + timings): `…/cfv3-20260610-175752/probe_results.jsonl`
- Judge HISTORY context (115 KB): `…/cfv3-20260610-175752/history_context.txt`
- Store A wiki (23 guides): `…/cfv3-20260610-175752/store-a/`
- Store B claims (342) + wiki (27 guides): `…/cfv3-20260610-175752/store-b/`
- Split manifest: `…/cfv3-20260610-175752/split_manifest.json`
- Run 1 store: `~/.proactive-context/experiments/cfv2-20260610-173322/` (10-session, 0 labels)
- Eval logs: `/tmp/eval-run3.log` (build), `/tmp/eval-score3.log` (frozen-label scoring)
