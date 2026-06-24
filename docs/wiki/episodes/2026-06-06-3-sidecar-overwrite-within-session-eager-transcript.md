---
type: episode-card
date: 2026-06-06
session: 9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd.jsonl
salience: architecture
status: active
subjects:
  - sidecar-naming
  - llm-transcript
  - req-id-overwrite
supersedes: []
related_claims: []
source_lines:
  - 1039-1134
captured_at: 2026-06-17T13:23:58Z
---

# Episode: Sidecar overwrite within session — eager transcript capture replaces lazy path storage

## Prior State

Initial design stored the sidecar file path and read it lazily when the user pressed Enter on a "Reading conversation" line, assuming each LLM call had a unique sidecar file.

## Trigger

Recording analysis revealed that all LLM calls in a capture session share the same sidecar filename ({req_id}-t1.json) because req_id is set once per session via init_context and stays constant, and turn stays 1 for single-shot calls. A later RECONCILE call overwrites the EXTRACT transcript ~20s after it's written. Also, the JSON path was wrong: messages are nested under request.messages, not top-level messages.

## Decision

Capture the transcript content eagerly the moment the EXTRACT llm.response event arrives (within the 100ms event drain, well before the ~20s overwrite window). Store the parsed string in a per-session HashMap in RunView, not the file path. Parse from request.messages with string content field.

## Consequences

- Transcript detail view shows the actual EXTRACT prompt, not a later RECONCILE prompt
- No dependency on sidecar file surviving to lazy-read time
- tui.rs detail modal (separate code path) still has the latent lazy-path bug — flagged in project memory

## Open Tail

- Root fix would be unique sidecar filenames per LLM call, but risks unbounded llm_turns/ growth product-wide

## Evidence

- transcript lines 1039-1134

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-3-sidecar-overwrite-within-session-eager-transcript.json`](transcripts/2026-06-06-3-sidecar-overwrite-within-session-eager-transcript.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-3-sidecar-overwrite-within-session-eager-transcript.json`](transcripts/raw/2026-06-06-3-sidecar-overwrite-within-session-eager-transcript.json)
