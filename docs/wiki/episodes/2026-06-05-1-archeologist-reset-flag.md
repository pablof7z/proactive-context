---
type: episode-card
date: 2026-06-05
session: 40a9b852-21b4-4d3d-8ca4-0e6d42650e61
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/40a9b852-21b4-4d3d-8ca4-0e6d42650e61.jsonl
salience: product
status: active
subjects:
  - archeologist-reset
  - capture-ledger
supersedes: []
related_claims: []
source_lines:
  - 78-439
captured_at: 2026-06-17T13:21:02Z
---

# Episode: archeologist-reset-flag

## Prior State

No mechanism to reset archeologist capture state. Deleting docs/wiki did not reset the 'new sessions' count because the capture ledger lived in ~/.proactive-context/captured-sessions/ — completely separate from the wiki output directory.

## Trigger

User deleted docs/wiki to start over but found the 'NEW:9' count unchanged, since session markers are stored independently from the wiki.

## Decision

Added `pc archeologist --reset` flag that removes capture markers so sessions count as new again. Supports `--project` for scoped per-project reset (same substring matching as a real run) and `--yes` to skip confirmation prompt. Short-circuits the entire pipeline: no scan, no LLM, no picker.

## Consequences

- Users can now properly start over after deleting wiki content
- The `--yes` flag semantics differ between reset (skip prompt) and normal run (mine all projects without picker); they are now allowed together for reset
- Non-TTY without --yes refuses to wipe rather than proceeding blind
- Per-project reset uses substring matching, so 'proactive-context' matches worktree dirs too (56 sessions across 3 dirs in testing)

## Open Tail

- The ledger and wiki remain in separate directories and can still drift — a future option could tie the ledger to the wiki dir so that deleting the wiki self-resets

## Evidence

- transcript lines 78-439

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-05-1-archeologist-reset-flag.json`](transcripts/2026-06-05-1-archeologist-reset-flag.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-05-1-archeologist-reset-flag.json`](transcripts/raw/2026-06-05-1-archeologist-reset-flag.json)
