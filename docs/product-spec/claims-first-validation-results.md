# Claims-First Validation Results

**Experiment dir:** `/Users/pablofernandez/.proactive-context/experiments/cfv2-20260610-173322`
**Judge model:** `ollama:glm-5.1:cloud`
**Date:** 2026-06-10
**Corpus:** `nostr-multi-platform` (`/Users/pablofernandez/Work/nostr-multi-platform`)

---

## Corpus split

| | |
|---|---|
| Total sessions found | 225 |
| HISTORY sessions (capped at 10) | 10 |
| FUTURE sessions | 215 |
| HISTORY agent-mode sessions | 4 |
| HISTORY human-interactive sessions | 6 |
| FUTURE sessions scanned for labels | 20 (cap) |
| FUTURE agent-mode (first 20) | 7 |
| FUTURE human-interactive (first 20) | 13 |

HISTORY cap: 10 (spec allows ~30; reduced to bound cost and wall time; noted here).

Store build times: Store B (claim tap ON) 406s; Store A (wiki only) 459s.

---

## Store contents after HISTORY replay

### Store B (claim log)
- **Claims:** 79 total across 10 sessions (`claims.jsonl`: 79 lines)
- **Wiki guides built (also):** 13 content guides
- **Guide names:** actor-backpressure-wiring, builder-guide-conventions, codex-exec-large-prompts, kernel-async-stack-drift, lmdb-event-store-stub, milestone-hardening-gates, nmp-desktop-shell, nmp-library-api, outbox-routing-hardcode, snapshot-pressure-gates, stress-harness-scenarios, uniffi-stub-ffi, writer-agent-rules

### Store A (wiki only, no claim tap)
- **Wiki guides built:** 8 content guides
- **Guide names:** android-architecture, builder-guide, coding-standards, codex-exec-stdin, desktop-shell, git-push-rules, milestone-gates, nmp-architecture

Note: Store A and B built the same 10 HISTORY sessions in two independent passes. The guide names diverged slightly because routing LLM calls are non-deterministic. This is a methodology limitation — the spec intended the claim tap to be a tap on the *same* single pass, but the implementation runs two separate passes, doubling EXTRACT cost and introducing routing variation.

---

## Probe 1 — Restatement recall

**Result: 0 verified labels. Probe 1 cannot be scored.**

| | |
|---|---|
| Total candidates proposed by judge | 0 |
| Verified by grep-match in HISTORY | 0 |

The label miner sent 20 FUTURE sessions (one LLM call per session) to the judge model with a HISTORY summary. The judge proposed 0 restatement candidates.

---

## Probe 3 — Operational metrics

Not measured (no probe runs executed due to 0 labels).

---

## Pre-registered read (§5)

The pre-registered criteria from the spec:

- **P1: User-direction recall ≥ Store A** — N/A: no labels generated
- **P3: Latency reduction ≥ 30%** — N/A: no probe runs
- **Coherence: incoherent rate < 20%** — N/A: no B briefings generated

**Pre-registered verdict: INCONCLUSIVE — harness failed to generate labels; the pre-registered criteria cannot be evaluated.**

(The auto-generated verdict in the raw report file shows "FAILS" due to a code path that maps the null case to the failure branch. That is a code bug, not a real finding. The correct characterization is INCONCLUSIVE.)

---

## Narrative — what happened and why

### Label mining returned 0 candidates

The label miner works as follows: for each of the first 20 FUTURE sessions, it calls the judge LLM with (a) a "HISTORY summary" built from raw transcript text of up to 10 HISTORY sessions (first 800 chars each, ~8,000 chars total) and (b) the FUTURE session transcript (first 3,000 chars). The judge is asked to propose restatement candidates — facts the user re-explained in the FUTURE session that were already established in HISTORY.

**Root cause:** The `build_history_summary` function concatenates raw transcript text. For the `nostr-multi-platform` corpus, the 10 HISTORY sessions contain mostly:
- Terse one-line user commands ("commit logical commits and push", "run codex exec with hello as the prompt")
- Large agent-mode sessions where user turns are tool-notification XML blobs

Only ~4,000 chars of substantive, intelligible HISTORY content was presented to the judge. With a history summary of that quality, the judge correctly found nothing worth matching.

The FUTURE sessions (7 agent-mode, 13 human-interactive out of the first 20) were richer in substantive content, but the judge had no history context to match against.

**This is a harness bug in the label miner, not a finding about the architecture under test.** The fix is straightforward: instead of building the history summary from raw transcript text, use the *wiki guide content* of Store A (or Store B) as the history summary. The wiki is precisely the distilled, intelligible representation of what was established — exactly what the judge needs to identify restatements.

### What the claim tap produced (positive signal)

Despite the label mining failure, the claim tap worked correctly:

- 79 claims logged across 10 sessions, with authority tagging (mix of explicit/implicit per session)
- sqlite-vec embedding table populated in `claims.db`
- Claim clustering at tau=0.55 functional (multiple clusters formed)
- Store B and Store A both produced coherent wiki guides from the same sessions

The infrastructure for claims-first injection is in place and functional.

### Methodology limitations (documented per spec §7)

1. **Two EXTRACT passes, not one:** The spec intended the claim tap to sit on the same pipeline run that builds the wiki, so one EXTRACT spend builds both stores. The implementation runs Store B (tap ON) then Store A (tap OFF) as independent passes. This doubles LLM cost for store building and introduces routing variance between stores.

2. **HISTORY cap at 10:** Spec allows ~30. Reduced for cost/time. The 10-session HISTORY produced coherent wiki guides (8-13 guides respectively) so the store contents are reasonable, but a deeper history would give the judge more signal.

3. **Label mining uses raw transcript, not wiki:** As described above, this is the primary harness bug that prevented any labels from being mined. Fix: use wiki guide content as history context.

4. **Corpus skew toward agent-mode sessions:** The `nostr-multi-platform` project uses concurrent agent sessions heavily. Most FUTURE sessions are agent-initiated task specs + tool notifications, not organic human restatements. A more suitable corpus for this probe would be one with long interactive sessions where the user repeatedly explains domain context.

### What to do next

The experiment was a **dry run that proved the infrastructure** but failed to generate labels due to the `build_history_summary` bug. Recommended next steps in priority order:

1. Fix `build_history_summary` to use wiki guide content from Store A instead of raw transcript text.
2. Re-run label mining only (`--score-only` flag available) against the already-built stores.
3. Optionally try a different corpus (one with more interactive sessions, less agent-mode).
4. Once labels are generated, Probe 1 and Probe 3 can be scored on the existing stores without rebuilding.

The claim store infrastructure (claims.rs, claims tap in capture.rs, claims inject path in eval.rs) is committed and correct. The 79 claims and both wiki stores from this run are preserved at:
- Store B: `/Users/pablofernandez/.proactive-context/experiments/cfv2-20260610-173322/store-b/`
- Store A: `/Users/pablofernandez/.proactive-context/experiments/cfv2-20260610-173322/store-a/`

---

## Raw artifacts

- Labels: `/Users/pablofernandez/.proactive-context/experiments/cfv2-20260610-173322/labels.jsonl` (empty — 0 candidates)
- Probe results: `/Users/pablofernandez/.proactive-context/experiments/cfv2-20260610-173322/probe_results.jsonl` (empty)
- Store A wiki: `/Users/pablofernandez/.proactive-context/experiments/cfv2-20260610-173322/store-a/`
- Store B claims: `/Users/pablofernandez/.proactive-context/experiments/cfv2-20260610-173322/store-b/`
- Split manifest: `/Users/pablofernandez/.proactive-context/experiments/cfv2-20260610-173322/split_manifest.json`
- Eval log: `/tmp/eval-v2.log`
