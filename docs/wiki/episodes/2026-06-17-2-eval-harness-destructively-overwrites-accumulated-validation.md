---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: root-cause
status: superseded
subjects:
  - eval-results-clobber
  - claims-first-validation
supersedes: []
related_claims: []
source_lines:
  - 1326-1344
  - 1372-1373
captured_at: 2026-06-17T13:28:48Z
---

# Episode: Eval harness destructively overwrites accumulated validation history

## Prior State

Running `pc eval` produces `claims-first-validation-results.md` — the canonical file accumulating results from multiple historical runs (Runs 11–12, ~1475 lines).

## Trigger

The baseline eval completed and auto-wrote this file, replacing 1475 lines of accumulated validation history with a 59-line single-run result. The overwrite was caught because the worktree tracked the prior version.

## Decision

Reverted the destructive overwrite to restore accumulated history. Flagged to the team via chat. No code fix applied — the eval harness still overwrites on every run.

## Consequences

- Any teammate running `pc eval` will hit the same destructive overwrite, destroying accumulated validation history
- The eval harness should append/version rather than overwrite — identified as a real bug requiring a fix

## Open Tail

- Eval harness needs a fix to preserve historical results (append or version the output file)

## Evidence

- transcript lines 1326-1344
- transcript lines 1372-1373

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-2-eval-harness-destructively-overwrites-accumulated-validation.json`](transcripts/2026-06-17-2-eval-harness-destructively-overwrites-accumulated-validation.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-2-eval-harness-destructively-overwrites-accumulated-validation.json`](transcripts/raw/2026-06-17-2-eval-harness-destructively-overwrites-accumulated-validation.json)
