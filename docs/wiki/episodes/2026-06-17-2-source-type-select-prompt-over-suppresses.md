---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: root-cause
status: superseded
subjects:
  - select-source-types
  - episode-cards
  - inject-prompt
supersedes: []
related_claims: []
source_lines:
  - 1801-1840
  - 1857-1914
captured_at: 2026-06-17T20:56:01Z
---

# Episode: Source-type SELECT prompt over-suppresses episode cards — tuned

## Prior State

Source-type SELECT guidance included a line 'Do NOT select a historical artifact as current truth' — a COMPILE-time presentation rule mis-applied at SELECT time, causing the selector to avoid picking episode cards even for history/trajectory queries

## Trigger

A2 eval arm showed episode selections dropped from 24→9 and trajectory recall dropped from 9/10→7/10 compared to A1; root cause identified as the misplaced COMPILE caution suppressing SELECT of the very artifact class that answers 'why did this change'

## Decision

Removed the suppressive 'Do NOT select a historical artifact' line from the SELECT preamble; rewrote block to explicitly retain every relevant episode card for why/history probes while still preferring guides for present-tense questions

## Consequences

- A2′ re-run launched against same frozen labels for validation
- A2's precision gain (contained 4→7) is preserved in intent but trajectory should recover toward A1's 9/10
- Separation of SELECT vs COMPILE concerns is now explicit in the prompt structure

## Open Tail

- A2′ results pending (~25 min background run); if trajectory recovers, A2 becomes a candidate for default-on alongside A1

## Evidence

- transcript lines 1801-1840
- transcript lines 1857-1914

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-2-source-type-select-prompt-over-suppresses.json`](transcripts/2026-06-17-2-source-type-select-prompt-over-suppresses.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-2-source-type-select-prompt-over-suppresses.json`](transcripts/raw/2026-06-17-2-source-type-select-prompt-over-suppresses.json)
