---
type: episode-card
date: 2026-06-06
session: 17c35740-f9e8-4b68-a281-400835f4c161
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/17c35740-f9e8-4b68-a281-400835f4c161.jsonl
salience: product
status: active
subjects:
  - archeologist-tui
  - feed-display
supersedes: []
related_claims: []
source_lines:
  - 482-484
  - 1181-1190
  - 1320-1380
  - 1418-1493
captured_at: 2026-06-17T13:35:23Z
---

# Episode: Archeologist TUI feed rewritten from pipeline jargon to human narration

## Prior State

The archeologist live feed displayed pipeline-internal jargon: 'create feed-avatar-hovercard', 'add feed-avatar-hovercard / Common Patterns', 'revise … [supersede]', plus internal stages like capture.extract, capture.authority_tagging, capture.route that are meaningless to end users. No way to drill down into what the model was sent or replied.

## Trigger

User called the feed 'utterly useless' and demanded natural-language narration ('I looked at conversation 123', 'I discovered this fact', 'Wrote guide abc') plus the ability to select a step and see the full prompt/completion content.

## Decision

Rewrote `feed_line_for_event` to emit human-readable lines: 'Reading conversation from 2026-06-05 (18 exchanges)', 'New guide: "rust-borrow-checker"', 'rust-borrow-checker › Common Patterns <text preview>'. Removed pipeline-internal events from feed. Added `detail` field to `FeedLine` and a full-screen detail overlay (Enter to open, Esc to close). `apply_reconcile_op` now emits wiki events with `text` excerpts; `capture.start` now carries `date` and `session_id`.

## Consequences

- Feed is human-readable instead of pipeline-internal
- Pipeline stages (extract, authority_tagging, route) still drive the 'current session' label but don't pollute the feed
- Each feed line carries a `detail` string for the drill-down overlay
- RunView tracks `last_sidecar` from `llm.response` events for future prompt/completion inspection
- capture.rs wiki event payloads now include `text` (truncated to 300 chars), enabling statement previews in the feed
- capture.start event now includes `date` and `session_id` fields

## Open Tail

- Sidecar-based full prompt+completion display in the detail overlay is tracked but not yet implemented — `last_sidecar` path is captured but not yet rendered

## Evidence

- transcript lines 482-484
- transcript lines 1181-1190
- transcript lines 1320-1380
- transcript lines 1418-1493

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-archeologist-tui-feed-rewritten-from-pipeline.json`](transcripts/2026-06-06-2-archeologist-tui-feed-rewritten-from-pipeline.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-archeologist-tui-feed-rewritten-from-pipeline.json`](transcripts/raw/2026-06-06-2-archeologist-tui-feed-rewritten-from-pipeline.json)
