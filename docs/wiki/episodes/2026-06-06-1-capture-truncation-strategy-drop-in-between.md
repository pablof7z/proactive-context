---
type: episode-card
date: 2026-06-06
session: 48ee4b84-0ddc-419e-8f94-1c5c75774d29
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/48ee4b84-0ddc-419e-8f94-1c5c75774d29.jsonl
salience: product
status: active
subjects:
  - capture-truncation
  - transcript-reduction
supersedes: []
related_claims: []
source_lines:
  - 574-734
captured_at: 2026-06-17T13:30:29Z
---

# Episode: Capture truncation strategy: drop in-between assistants, never user turns

## Prior State

Long sessions were truncated by keeping the tail (last 200k/250k chars), silently dropping the head — where user requirements, spec decisions, and framing typically live. This systematically caused EXTRACT to miss load-bearing product direction in long sessions, contributing to sparse wiki coverage.

## Trigger

User directive: 'for truncation we should prefer to remove the in-between assistant messages, never user messages — by in-between I mean assistant messages that are followed by assistant messages.' Preceded by investigation showing tail-truncation loses requirement-bearing content and tool-IO stripping compounds sparseness.

## Decision

Replace byte-level tail-slicing with turn-level reduction: `reduce_turns_to_fit` drops only consecutive ('in-between') assistant turns, keeping all user turns and the final assistant turn per run. A `tail_capped` char-boundary-safe backstop exists for pathological cases where surviving content alone exceeds budget.

## Consequences

- User turns are always preserved in capture input regardless of session length
- Long sessions no longer lose their requirement-bearing head to truncation
- Citation integrity preserved: `transcript_lines`, `transcript_roles`, and `numbered_transcript` all derive from the same reduced set, maintaining absolute-line-number invariants for `evidence_is_valid` / `author_for_ranges` / `cite`
- `tail_capped` backstop only fires in pathological cases (user-only content exceeding budget)
- Directly addresses the 'sparse wiki from 253 conversations' investigation by preserving the most informative transcript content
- The `numbered` flag in `reduce_turns_to_fit` accounts for the `NNNN| ` per-line prefix overhead in the budget model, preventing the backstop from re-trimming the head

## Open Tail

- Peer investigation into 'only ~33 of 61+ sessions captured' is still open — this change is orthogonal to mark_captured-on-timeout and archeologist-routing work

## Evidence

- transcript lines 574-734

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-capture-truncation-strategy-drop-in-between.json`](transcripts/2026-06-06-1-capture-truncation-strategy-drop-in-between.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-capture-truncation-strategy-drop-in-between.json`](transcripts/raw/2026-06-06-1-capture-truncation-strategy-drop-in-between.json)
