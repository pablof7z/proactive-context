---
type: episode-card
date: 2026-06-06
session: 9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd.jsonl
salience: product
status: active
subjects:
  - feed-scroll
  - live-update-drift
  - position-indicator
supersedes: []
related_claims: []
source_lines:
  - 1217-1275
  - 1292-1349
captured_at: 2026-06-29T12:17:25Z
---

# Episode: Feed scroll position held during live updates with position indicator

## Prior State

The live-update path only held scroll position when the feed was paused (if view.feed_paused && !was_at_bottom). When scrolled up during a live run without pausing, every incoming event grew the feed but left feed_scroll unchanged — the cursor drifted to a different line and the view slid out from under the user. There was no on-screen position readout.

## Trigger

User reported: 'when I scroll past the viewport I don't see the position since the container doesn't scroll down'. Advisor confirmed the static window math was provably correct; the real culprit was the live-drift path.

## Decision

Hold scroll position whenever scrolled up (not just paused): change condition to 'if view.feed_paused || !was_at_bottom', bumping feed_scroll in lock-step with the growing feed so the cursor stays on the same logical line. Add a position indicator ('feed · line N/M') to the feed title whenever scrolled up. Extract the window math into a testable feed_window() helper locked by an invariant test.

## Consequences

- Cursor stays on the same logical line during live updates without pausing
- Position indicator shows exact cursor location while scrolled up
- feed_window() helper is unit-tested with an invariant test, preventing future re-litigation of the window math
- Live-follow resumes when user scrolls back to the bottom

## Open Tail

- Fix only bites during a live run; post-run scrolling was already correct but benefits from the position indicator

## Evidence

- transcript lines 1217-1275
- transcript lines 1292-1349

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-4-feed-scroll-position-held-during-live.json`](transcripts/2026-06-06-4-feed-scroll-position-held-during-live.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-4-feed-scroll-position-held-during-live.json`](transcripts/raw/2026-06-06-4-feed-scroll-position-held-during-live.json)
