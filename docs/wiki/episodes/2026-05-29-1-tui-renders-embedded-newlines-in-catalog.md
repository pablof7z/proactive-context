---
type: episode-card
date: 2026-05-29
session: 31cc17e6-16bf-4c63-a6b4-1b1d67795aa1
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/31cc17e6-16bf-4c63-a6b4-1b1d67795aa1.jsonl
salience: product
status: active
subjects:
  - tui-modal-rendering
  - catalog-display
supersedes: []
related_claims: []
source_lines:
  - 1-448
captured_at: 2026-06-17T12:40:27Z
---

# Episode: TUI renders embedded newlines in catalog and payload display

## Prior State

Catalog and other multiline content displayed `
` escape sequences as literal text in the TUI modal; JSON payloads with embedded newlines rendered them inline rather than as visual line breaks. Payload display was capped at 8 lines, insufficient for expanded multiline content.

## Trigger

User reported that line separators in the catalog must render as actual newlines in the tail TUI, not as escaped sequences.

## Decision

Updated TUI rendering to split on `\n` escape sequences in JSON payloads, respect actual newlines in sidecar message content before character chunking, and increased the payload display limit from 8 to 25 lines. Added modal vertical scrolling to handle expanded content.

## Consequences

- Catalog entries now display with proper visual line separation in the TUI
- Sidecar messages (select/compile turns) also render multiline content correctly
- Modal scroll state and scroll-up/scroll-down keybindings added to navigate longer expanded content
- Display limit raised from 8 to 25 lines to accommodate expanded multiline payloads

## Open Tail

*(none)*

## Evidence

- transcript lines 1-448

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-tui-renders-embedded-newlines-in-catalog.json`](transcripts/2026-05-29-1-tui-renders-embedded-newlines-in-catalog.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-tui-renders-embedded-newlines-in-catalog.json`](transcripts/raw/2026-05-29-1-tui-renders-embedded-newlines-in-catalog.json)
