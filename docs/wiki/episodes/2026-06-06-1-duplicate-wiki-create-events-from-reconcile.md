---
type: episode-card
date: 2026-06-06
session: 9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd.jsonl
salience: root-cause
status: active
subjects:
  - capture-reconcile
  - wiki-create-event
  - feed-duplication
supersedes: []
related_claims: []
source_lines:
  - 56-58
  - 611-612
  - 796-800
captured_at: 2026-06-17T13:23:58Z
---

# Episode: Duplicate wiki.create events from reconcile op labels

## Prior State

The reconcile stage emitted wiki.create based on the LLM's op label (op.op == "create"), not on whether a guide was genuinely created. When the LLM labeled all N statements of a brand-new guide as "create", all N emitted wiki.create — producing 10 duplicate "New guide" feed lines for one actual creation.

## Trigger

User observed "Shows New guide a bunch of times, but no claims" in the TUI feed; code analysis at capture.rs:2049 confirmed the event name was chosen by op label alone.

## Decision

Detect genuine creation inside the locked closure via existing.is_none(), set a flag, and emit wiki.create only on actual creation. On error, skip the event entirely. Other "create"-labeled ops fall through to add-statement path and emit wiki.add_statement instead.

## Consequences

- 10 duplicate "New guide" lines collapse to 1 New guide + 9 claim lines
- Feed math (captured/seen/triage-skip) now matches actual guide creation count

## Open Tail

*(none)*

## Evidence

- transcript lines 56-58
- transcript lines 611-612
- transcript lines 796-800

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-duplicate-wiki-create-events-from-reconcile.json`](transcripts/2026-06-06-1-duplicate-wiki-create-events-from-reconcile.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-duplicate-wiki-create-events-from-reconcile.json`](transcripts/raw/2026-06-06-1-duplicate-wiki-create-events-from-reconcile.json)
