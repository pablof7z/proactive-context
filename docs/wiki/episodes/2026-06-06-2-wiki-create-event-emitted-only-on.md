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
  - event-semantics
supersedes: []
related_claims: []
source_lines:
  - 589-611
  - 796-806
  - 1201-1202
captured_at: 2026-06-29T12:17:25Z
---

# Episode: wiki.create event emitted only on genuine guide creation

## Prior State

The event name at capture.rs:2049 was chosen by the reconcile LLM's op label (op.op == 'create' vs 'add'). When the LLM labeled all N statements of a brand-new guide as 'create', the first genuinely created the guide and the rest fell through to the add-statement path — but all N still emitted wiki.create, producing duplicate 'New guide ×10' feed lines and hiding the actual claim lines.

## Trigger

User complaint: 'Shows New guide a bunch of times, but no claims'. Code investigation confirmed the event name was tied to the op label, not to whether a guide was actually created.

## Decision

Detect genuine creation inside the with_guide_locked closure via a flag set when existing.is_none(). Emit wiki.create only when the flag is set; skip the event entirely on error. Statements labeled 'create' that find an existing guide now emit wiki.add_statement instead.

## Consequences

- Duplicate 'New guide' feed lines collapse to 1 create + N add-statement lines
- Claim lines are now visible in the feed instead of being masked by duplicate create events
- Fix was committed to capture.rs and swept into master by a peer's merge before this session's commit

## Open Tail

*(none)*

## Evidence

- transcript lines 589-611
- transcript lines 796-806
- transcript lines 1201-1202

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-wiki-create-event-emitted-only-on.json`](transcripts/2026-06-06-2-wiki-create-event-emitted-only-on.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-wiki-create-event-emitted-only-on.json`](transcripts/raw/2026-06-06-2-wiki-create-event-emitted-only-on.json)
