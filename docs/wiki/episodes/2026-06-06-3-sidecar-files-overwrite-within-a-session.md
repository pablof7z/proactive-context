---
type: episode-card
date: 2026-06-06
session: 9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd.jsonl
salience: product
status: active
subjects:
  - sidecar-overwrite
  - llm-turn-logging
  - transcript-drill-down
supersedes: []
related_claims: []
source_lines:
  - 1039-1097
  - 1126-1151
  - 1197-1215
captured_at: 2026-06-29T12:17:25Z
---

# Episode: Sidecar files overwrite within a session — eager transcript capture required

## Prior State

Sidecar JSON files (full prompt+completion per LLM call) were assumed to persist per-call, so storing the sidecar path and reading it lazily later would yield the correct transcript. The transcript drill-down feature was initially designed around lazy path-storage.

## Trigger

Investigation of a real recording found every LLM call in a capture session shares the same sidecar filename ({req_id}-t1.json) because req_id is set once per init_context and stays constant for all of a session's LLM calls, and turn stays 1 for single-shot calls. Later reconcile calls overwrite the EXTRACT transcript ~20s after it's written. A real sidecar file on disk was confirmed to contain a RECONCILE prompt, not the EXTRACT transcript.

## Decision

Capture transcript content eagerly at the moment the EXTRACT llm.response event arrives (insert-if-absent per session), storing the parsed string in memory rather than the file path. The ~21s overwrite gap vs the TUI's 100ms drain makes this safe. Fix the JSON parser path from top-level 'messages' to 'request.messages' to match the actual sidecar structure.

## Consequences

- Transcript drill-down reliably shows the EXTRACT prompt, not the last reconcile prompt
- The same latent bug exists in tui.rs's detail modal (lazy path read → overwritten content for completed turns) — flagged in memory but not fixed
- Root fix (unique sidecar filenames per call) was deferred because it risks unbounded llm_turns/ disk growth product-wide
- Finding recorded in memory file llm-sidecar-overwrite-within-session.md for future reference

## Open Tail

- tui.rs detail modal still has the sidecar-overwrite latent bug
- Unique sidecar filenames would fix the root cause but introduces disk growth concerns

## Evidence

- transcript lines 1039-1097
- transcript lines 1126-1151
- transcript lines 1197-1215

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-3-sidecar-files-overwrite-within-a-session.json`](transcripts/2026-06-06-3-sidecar-files-overwrite-within-a-session.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-3-sidecar-files-overwrite-within-a-session.json`](transcripts/raw/2026-06-06-3-sidecar-files-overwrite-within-a-session.json)
