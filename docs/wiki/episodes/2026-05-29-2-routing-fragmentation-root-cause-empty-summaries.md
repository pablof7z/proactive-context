---
type: episode-card
date: 2026-05-29
session: 26c909a1-6c07-4761-bac5-6e880cd7a063
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/26c909a1-6c07-4761-bac5-6e880cd7a063.jsonl
salience: root-cause
status: active
subjects:
  - capture-route-stage
  - wiki-fragmentation
  - guide-altitude
supersedes: []
related_claims: []
source_lines:
  - 3831-3856
  - 4006-4045
  - 4153-4180
captured_at: 2026-06-29T11:00:57Z
---

# Episode: Routing fragmentation root cause: empty summaries + no altitude definition

## Prior State

The ROUTE stage of the capture pipeline had no definition of guide 'altitude' (what scope a guide covers — subsystem vs fact), and new guides were created with empty summaries, so each session's ROUTE was blind to what prior guides already covered. This produced severe fragmentation: 33 guides from half the corpus vs gold's 27, with 5 citation-* guides and 4 compile/librarian-* guides where gold had 1 each.

## Trigger

Archeologist test (bulk reprocessing of 47 sessions through the pipeline) confirmed for the third time that routing — not reconciliation — is the actual bottleneck. The precise root cause was identified: not synonym confusion as hypothesized, but empty summaries + no guide-altitude definition, causing the model to create one guide per fact.

## Decision

Define a guide as a subsystem-level chapter and thread real titles/summaries into the index that ROUTE reads. Committed as 8d149c0 (capture.rs only, build green).

## Consequences

- Fragmentation genuinely collapsed: citation-* 5→1, compile/librarian 4→1, 0 empty summaries, 27 guides matching gold's count
- Supersession breadcrumbs verified working (Redis→Postgres reversal renders '(Previously: all user sessions were stored in Redis)')
- Guides re-oversplit in the iter3 run (27→35) after the authority tag change, confirming routing remains the recurring bottleneck independent of the authority model
- Three bug-fixes verified and committed: d8e610f (output-dir safety leak), f8db3b5 (too-short gate), 8d149c0 (ROUTE) — all on master, unpushed

## Open Tail

- Routing over-split recurred in iter3 (35 guides vs gold 27) — a concurrent session is working this but it's not yet resolved
- ROUTE root cause fix is confirmed but the deeper routing quality (matching gold's 0.61 guides/session ratio) still needs work

## Evidence

- transcript lines 3831-3856
- transcript lines 4006-4045
- transcript lines 4153-4180

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-routing-fragmentation-root-cause-empty-summaries.json`](transcripts/2026-05-29-2-routing-fragmentation-root-cause-empty-summaries.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-routing-fragmentation-root-cause-empty-summaries.json`](transcripts/raw/2026-05-29-2-routing-fragmentation-root-cause-empty-summaries.json)
