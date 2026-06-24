---
type: episode-card
date: 2026-06-06
session: 5a1472ae-2784-423d-8681-0bedcf6c165f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5a1472ae-2784-423d-8681-0bedcf6c165f.jsonl
salience: root-cause
status: active
subjects:
  - capture-extract
  - extract-preamble
  - wiki-index-in-extract
supersedes: []
related_claims: []
source_lines:
  - 100-105
  - 318-375
  - 396-405
  - 570-579
captured_at: 2026-06-17T13:39:33Z
---

# Episode: EXTRACT stage validated; wiki-index-in-EXTRACT rejected; sweep nudge adopted

## Prior State

Hypothesis that EXTRACT systematically drops important facts from transcripts, and that providing the existing wiki index to EXTRACT would improve claim coverage and routing accuracy.

## Trigger

Empirical A/B investigation across ~20 runs (3 conditions: original prompt, +sweep nudge, +wiki index) on real session transcripts using kimi-k2.6.

## Decision

Three concurrent decisions on the EXTRACT stage: (1) Added a narrow sweep-completeness nudge to EXTRACT_PREAMBLE — 'read the whole transcript top to bottom, capture late-session reversals' — without pushing finer claim granularity (which would feed the known over-splitting bottleneck at ROUTE). (2) Rejected wiki index in live EXTRACT — it added output variance and produced complete extraction failures (0 claims) from JSON truncation at the 6000-token output cap, with no coverage gain. (3) Deliberately did NOT push for more granular/atomic claim splitting, citing the project's known over-splitting failure mode.

## Consequences

- PC_EXTRACT_NO_GRANULARITY=1 env var available to A/B the original prompt against the nudge version
- Wiki index remains available via --wiki-dir flag in debug mode only, not wired into live capture
- Late-session reversals remain intermittently missed across all conditions — an acknowledged marginal gap
- EXTRACT is confirmed NOT to systematically drop load-bearing facts; the Rust evidence-verification gate drops nothing silently
- A 135KB .jsonl collapses to ~23 lines of prose after parse_transcript strips tool calls — tool-heavy sessions present far less to EXTRACT than file size suggests
- Debug instrumentation added: pc debug transcript and pc debug extract now surface JSON parse failures that the live path silently swallows as 0 claims

## Open Tail

- The 6000-token output cap on EXTRACT may be tight for dense sessions (~60-80 claims); raising it is an option but wasn't tested
- The 0-claim failures with wiki index may have been nondeterminism rather than truncation — raw responses weren't inspected to distinguish truncation from garbage
- Debug preprocess initially used byte-tail truncation instead of reduce_turns_to_fit (fixed post-merge); any future debug commands must mirror live path's turn-reduction strategy

## Evidence

- transcript lines 100-105
- transcript lines 318-375
- transcript lines 396-405
- transcript lines 570-579

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-extract-stage-validated-wiki-index-in.json`](transcripts/2026-06-06-1-extract-stage-validated-wiki-index-in.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-extract-stage-validated-wiki-index-in.json`](transcripts/raw/2026-06-06-1-extract-stage-validated-wiki-index-in.json)
