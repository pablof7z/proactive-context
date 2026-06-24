---
type: episode-card
date: 2026-05-28
session: 5cf47d01-7a4e-4052-9948-8878a21b5b6a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5cf47d01-7a4e-4052-9948-8878a21b5b6a.jsonl
salience: root-cause
status: active
subjects:
  - chunker
  - utf8-handling
supersedes: []
related_claims: []
source_lines:
  - 304-317
captured_at: 2026-06-17T12:12:17Z
---

# Episode: Chunker panics on multi-byte UTF-8 characters

## Prior State

The take_overlap function in chunker.rs sliced text at byte offsets that could fall inside multi-byte UTF-8 characters (e.g. em-dash '—' at byte 542).

## Trigger

Runtime panic: 'byte index 544 is not a char boundary; it is inside — (bytes 542..545)' when indexing markdown containing non-ASCII text.

## Decision

take_overlap now walks back to the nearest valid char boundary before slicing, ensuring all string slices are on UTF-8 boundaries.

## Consequences

- Chunker no longer panics on content with em-dashes, smart quotes, or other multi-byte characters
- Any non-ASCII markdown content is now chunkable without crashes

## Open Tail

*(none)*

## Evidence

- transcript lines 304-317

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-2-chunker-panics-on-multi-byte-utf.json`](transcripts/2026-05-28-2-chunker-panics-on-multi-byte-utf.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-2-chunker-panics-on-multi-byte-utf.json`](transcripts/raw/2026-05-28-2-chunker-panics-on-multi-byte-utf.json)
