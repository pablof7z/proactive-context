---
type: episode-card
date: 2026-06-06
session: 9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd.jsonl
salience: product
status: active
subjects:
  - feed-windowing
  - scroll-drift
  - position-indicator
supersedes: []
related_claims: []
source_lines:
  - 1217-1348
captured_at: 2026-06-17T13:23:58Z
---

# Episode: Feed scroll position drift on live updates — bottom-anchored windowing + position indicator

## Prior State

The feed window's bottom edge was tied to scroll offset (end = total - feed_scroll), so pressing Up peeled rows off the bottom and the cursor could land outside the rendered window. During live runs without pause, incoming events grew the feed but left feed_scroll unchanged, causing the cursor to drift to a different line. There was no on-screen position readout.

## Trigger

User: "when I scroll past the viewport I don't see the position since the container doesn't scroll down." Advisor confirmed window math was provably correct for static case — the real bug was the live-update path only holding position when paused.

## Decision

Rewrite windowing as bottom-anchored (cursor moves within window; window scrolls only when cursor reaches top). Extract feed_window() helper with invariant tests. Fix live-update drift: hold scroll position when not at bottom (not just when paused) by bumping feed_scroll in lock-step with growing feed. Add position indicator "feed · line N/M" to feed title whenever scrolled up.

## Consequences

- Scroll position holds during live updates — cursor stays on the same logical line as new events arrive
- Position indicator visible when scrolled, confirming where you are
- feed_window() invariant test locks the window math against regression

## Open Tail

*(none)*

## Evidence

- transcript lines 1217-1348

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-4-feed-scroll-position-drift-on-live.json`](transcripts/2026-06-06-4-feed-scroll-position-drift-on-live.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-4-feed-scroll-position-drift-on-live.json`](transcripts/raw/2026-06-06-4-feed-scroll-position-drift-on-live.json)
