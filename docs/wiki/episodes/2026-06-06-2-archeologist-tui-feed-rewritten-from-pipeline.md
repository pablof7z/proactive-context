---
type: episode-card
date: 2026-06-06
session: 17c35740-f9e8-4b68-a281-400835f4c161
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/17c35740-f9e8-4b68-a281-400835f4c161.jsonl
salience: product
status: active
subjects:
  - archeologist-tui
  - feed-line
  - detail-pane
  - event-payloads
supersedes:
  - 2026-06-06-1-archeologist-tui-narrates-capture-lifecycle-during
related_claims: []
source_lines:
  - 482-484
  - 1138-1186
  - 1596-1613
captured_at: 2026-06-29T12:42:26Z
---

# Episode: Archeologist TUI feed rewritten from pipeline jargon to human-readable narration with drill-down

## Prior State

The archeologist live feed displayed technical pipeline-internal events (capture.extract, capture.authority_tagging, capture.route, agent_done) as cryptic log lines like 'create feed-avatar-hovercard' and 'add slug / section'. The staged reconcile pipeline (apply_reconcile_op) mutated wiki files directly without emitting any log_event calls, so those operations were invisible to the feed. No ability to inspect what was sent to the model or what it replied.

## Trigger

User saw the TUI and said it was 'utterly useless' — a log of 'a bunch of bullshit'. User wanted natural-language narration ('I looked at conversation 123', 'I discovered this fact', 'Wrote guide abc') and the ability to select and open feed entries to see exactly what was sent to the model and what it replied.

## Decision

Feed completely rewritten: pipeline-internal events removed from feed (still drive the 'current stage' label); remaining events rendered as natural-language sentences. apply_reconcile_op now emits wiki.create/wiki.add_statement/wiki.revise_statement events with slug, title, section, and a 300-char text excerpt. capture.start enriched with date and session_id. FeedLine gains a detail field; RunView gains detail_open and last_sidecar; Enter opens a full-screen detail overlay, Esc closes it; llm.response sidecar path tracked for future drill-down.

## Consequences

- apply_reconcile_op now emits log_event for create/add/revise ops — these were previously silent direct file mutations invisible to the TUI
- capture.start payload now includes date and session_id (were available in scope but not logged)
- wiki.add_statement and wiki.revise_statement event payloads now include a 300-char text excerpt
- FeedLine struct gains detail: String field; RunView gains detail_open: Option<String> and last_sidecar: Option<String>
- User can scroll the feed with arrow keys, highlight a line, press Enter to see full detail text, Esc to close
- Pipeline-internal events (capture.extract, capture.authority_tagging, capture.route) are filtered out of the feed but still update the 'current session' stage label

## Open Tail

- Sidecar drill-down (showing full LLM prompt + completion) is tracked via last_sidecar but the detail pane does not yet read and render the sidecar JSON file content

## Evidence

- transcript lines 482-484
- transcript lines 1138-1186
- transcript lines 1596-1613

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-archeologist-tui-feed-rewritten-from-pipeline.json`](transcripts/2026-06-06-2-archeologist-tui-feed-rewritten-from-pipeline.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-archeologist-tui-feed-rewritten-from-pipeline.json`](transcripts/raw/2026-06-06-2-archeologist-tui-feed-rewritten-from-pipeline.json)
