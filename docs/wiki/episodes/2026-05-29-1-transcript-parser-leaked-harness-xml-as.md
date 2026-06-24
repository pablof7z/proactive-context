---
type: episode-card
date: 2026-05-29
session: 0cbfa1f3-ca48-4660-be42-8f15c75e7c95
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/0cbfa1f3-ca48-4660-be42-8f15c75e7c95.jsonl
salience: root-cause
status: active
subjects:
  - transcript-parse
  - extract-text
  - harness-xml-filter
supersedes: []
related_claims: []
source_lines:
  - 75-156
  - 157-296
captured_at: 2026-06-17T12:37:03Z
---

# Episode: Transcript parser leaked harness XML as human turns

## Prior State

`extract_text` in transcript.rs correctly filtered typed array blocks (skipping `tool_result` via `type` field), but when Claude Code stored harness-injected content as a plain string rather than a typed block array, it passed through verbatim. `<task-notification>`, `<system-reminder>`, and other harness XML was treated as human conversation text and included in `recent_context_text`.

## Trigger

User observed garbage in event.jsonl: a `retrieve.subquery` event whose `text` field contained raw `<tool-use-id>` / `<output-file>` / `<status>` XML. Investigation revealed Claude Code injects `<task-notification>` as `role: "user"` plain-string messages — harness-to-model signals, not human turns.

## Decision

In `extract_text`, skip any plain-string content that starts with `<`, since human messages never begin with XML tags. This replaces the narrower initial fix (sentinel `<tool-use-id>` detection) with a general rule covering `<task-notification>`, `<system-reminder>`, and any future harness XML.

## Consequences

- Vector embedder no longer receives harness XML as semantic queries (was producing garbage embeddings)
- Selector and compiler LLM calls no longer receive harness XML noise in their `recent` preamble
- All typed-array tool_result blocks were already filtered correctly; only the plain-string path was broken
- The `starts_with('<')` heuristic relies on the invariant that human messages don't begin with XML tags

## Open Tail

*(none)*

## Evidence

- transcript lines 75-156
- transcript lines 157-296

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-transcript-parser-leaked-harness-xml-as.json`](transcripts/2026-05-29-1-transcript-parser-leaked-harness-xml-as.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-transcript-parser-leaked-harness-xml-as.json`](transcripts/raw/2026-05-29-1-transcript-parser-leaked-harness-xml-as.json)
