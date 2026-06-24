---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: root-cause
status: active
subjects:
  - pc-eval
  - claims-first-validation-results
supersedes:
  - 2026-06-17-2-eval-harness-destructively-overwrites-accumulated-validation
related_claims: []
source_lines:
  - 1326-1374
captured_at: 2026-06-17T18:15:38Z
---

# Episode: Eval harness clobbers accumulated validation history

## Prior State

Running `pc eval` was assumed safe — it would produce new results without destroying prior work.

## Trigger

Baseline eval run auto-wrote claims-first-validation-results.md, replacing ~1475 lines of accumulated Runs 11–12 history with a 59-line single-run baseline.

## Decision

Identified as a real bug in the eval harness: it overwrites the results file instead of appending or versioning. Immediately reverted the clobber and flagged to the team.

## Consequences

- Anyone running pc eval will hit the same overwrite — unsafe to re-run without backup
- Eval harness must be changed to append/version before it's safe for routine use

## Open Tail

- Eval harness overwrite fix not yet implemented

## Evidence

- transcript lines 1326-1374

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-3-eval-harness-clobbers-accumulated-validation-history.json`](transcripts/2026-06-17-3-eval-harness-clobbers-accumulated-validation-history.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-3-eval-harness-clobbers-accumulated-validation-history.json`](transcripts/raw/2026-06-17-3-eval-harness-clobbers-accumulated-validation-history.json)
