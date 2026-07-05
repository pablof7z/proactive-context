---
type: episode-card
date: 2026-06-06
session: 48ee4b84-0ddc-419e-8f94-1c5c75774d29
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/48ee4b84-0ddc-419e-8f94-1c5c75774d29.jsonl
salience: product
status: active
subjects:
  - capture-pipeline
  - transcript-truncation
  - wiki-extraction
supersedes: []
related_claims: []
source_lines:
  - 574-726
captured_at: 2026-06-29T12:34:22Z
---

# Episode: Capture truncation strategy: tail-slice replaced by in-between-assistant dropping

## Prior State

Long sessions exceeding the capture budget (200k chars for triage, 250k for EXTRACT) were truncated by keeping only the tail — a raw byte-slice of the last N chars. This silently dropped the head of long sessions, where user requirements, spec decisions, and framing typically live, causing systematic loss of load-bearing product direction.

## Trigger

User directive: 'for truncation we should prefer to remove the in-between assistant messages, never user messages — by in-between I mean assistant messages that are followed by assistant messages.' This was surfaced during an audit of the capture pipeline's filtering/truncation chain, which identified tail-only truncation as a prime suspect in the 'wiki looks sparse from 253 conversations' investigation.

## Decision

Replaced the tail-slice truncation with a turn-level reduction strategy: `reduce_turns_to_fit` drops only 'in-between' assistant turns (assistant turns immediately followed by another assistant turn — i.e. tool-call narration / intermediate steps), oldest-first, and only when over budget. User turns are never dropped. The final assistant turn of each run is always kept. A char-boundary-safe `tail_capped` backstop replaces the raw byte-slice (which could panic mid-codepoint) and fires only in the pathological case where surviving content alone exceeds budget.

## Consequences

- Both triage (200k) and EXTRACT (250k) truncation sites now use the same turn-level reduction, ensuring the user's initial requirements and framing survive into capture.
- The numbered_transcript, transcript_lines, and transcript_roles all derive from the same reduced turn set, preserving the absolute-line-number invariant that evidence_is_valid, author_for_ranges, and cite depend on.
- The `numbered` flag in the budget model accounts for the ~6-9 char/line `NNNN| ` prefix overhead so the backstop doesn't re-trim the head after turn-level reduction.
- Raw byte-slice truncation that could panic on multi-byte characters (emoji) is replaced with char-boundary-safe tail_capped.
- Directly addresses the 'sparse wiki from long sessions' problem — long sessions that lost their requirement-bearing head to tail-truncation now retain it.
- Orthogonal to concurrent mark_captured-on-timeout and archeologist-routing work; no overlap with peers' efforts.

## Open Tail

- The peer investigating 'only ~33 of 61+ sessions captured' should measure how many session files exceed 250k chars to quantify the head-truncation loss this fix addresses.
- Tool I/O is still stripped (text-blocks-only in extract_text) — sessions that did real work through tool calls with terse prose remain thin to EXTRACT.

## Evidence

- transcript lines 574-726

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-capture-truncation-strategy-tail-slice-replaced.json`](transcripts/2026-06-06-1-capture-truncation-strategy-tail-slice-replaced.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-capture-truncation-strategy-tail-slice-replaced.json`](transcripts/raw/2026-06-06-1-capture-truncation-strategy-tail-slice-replaced.json)
