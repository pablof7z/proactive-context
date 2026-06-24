---
type: episode-card
date: 2026-06-18
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
salience: root-cause
status: active
subjects:
  - mark-captured-bug
  - session-timeout
supersedes:
  - 2026-06-06-1-mark-captured-runs-unconditionally-on-timeout
related_claims: []
source_lines:
  - 210-212
captured_at: 2026-06-18T20:43:53Z
---

# Episode: mark_captured Timeout Bug Permanently Drops Sessions

## Prior State

Timed-out capture sessions were assumed to be retried or handled gracefully

## Trigger

Investigation of sparse transcript coverage revealed sessions marked as captured despite producing zero output

## Decision

Identified that mark_captured runs unconditionally on timeout, permanently marking timed-out sessions as already-captured with no output, excluding them from all future archeologist runs

## Consequences

- Sessions that timed out during capture produce no cards and no transcripts — silent data loss
- Recovery requires pc archeologist reset then re-run
- Transcript date histogram clusters on archeologist-run days rather than spreading evenly, reflecting this bug's footprint

## Open Tail

- Whether to fix mark_captured to only mark on successful completion
- Whether existing permanently-dropped sessions can be identified and recovered

## Evidence

- transcript lines 210-212

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-mark-captured-timeout-bug-permanently-drops.json`](transcripts/2026-06-18-2-mark-captured-timeout-bug-permanently-drops.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-mark-captured-timeout-bug-permanently-drops.json`](transcripts/raw/2026-06-18-2-mark-captured-timeout-bug-permanently-drops.json)
