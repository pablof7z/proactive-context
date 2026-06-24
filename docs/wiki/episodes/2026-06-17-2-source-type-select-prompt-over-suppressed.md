---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: root-cause
status: active
subjects:
  - select-source-types-prompt
  - episode-card-selection
supersedes:
  - 2026-06-17-2-source-type-select-prompt-over-suppresses
related_claims: []
source_lines:
  - 1857-1890
  - 1928-1933
  - 1987-2001
captured_at: 2026-06-17T22:04:35Z
---

# Episode: Source-type SELECT prompt over-suppressed episode cards — root cause found and fixed

## Prior State

No content-kind guidance in the SELECT preamble; the selector had no type hints and treated all catalog rows uniformly.

## Trigger

A2 arm eval showed episode-card selections dropped from 22→9 and trajectory recall fell from 9/10→7/10, meaning the source-type guidance was over-suppressing exactly the artifact class that answers 'why did this change.'

## Decision

Diagnosed root cause: the line 'Do NOT select a historical artifact as CURRENT truth' was a COMPILE-time presentation rule mis-applied at SELECT time, making the selector hesitant to pick episodes at all. Removed that line and rewrote `SELECT_SOURCE_TYPES_BLOCK` to explicitly retain every relevant episode card for why/history probes while still preferring guides for present-tense questions.

## Consequences

- A2′ recovered episode-card selections from 9 back to 25 (matching baseline and A1).
- Trajectory rose to 8/10 (≥ baseline), stale-leak stayed 0.
- The fix is mechanistically confirmed by deterministic selection counts (not judge-dependent).

## Open Tail

- A2′ recall impact still unclear due to judge variance — high-power eval pending.

## Evidence

- transcript lines 1857-1890
- transcript lines 1928-1933
- transcript lines 1987-2001

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-2-source-type-select-prompt-over-suppressed.json`](transcripts/2026-06-17-2-source-type-select-prompt-over-suppressed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-2-source-type-select-prompt-over-suppressed.json`](transcripts/raw/2026-06-17-2-source-type-select-prompt-over-suppressed.json)
