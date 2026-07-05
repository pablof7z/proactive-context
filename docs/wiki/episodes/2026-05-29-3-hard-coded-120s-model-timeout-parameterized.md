---
type: episode-card
date: 2026-05-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
salience: root-cause
status: active
subjects:
  - call-model-blocking-timeout
  - doctor-batch-timeout
  - hot-path-latency
supersedes: []
related_claims: []
source_lines:
  - 10529-10566
captured_at: 2026-06-29T11:12:26Z
---

# Episode: Hard-coded 120s model timeout parameterized to separate hot-path from batch jobs

## Prior State

call_model_blocking had a hard-coded 120s HTTP timeout shared across all callers: capture's open-question gen, triage, awareness, and doctor. Whole-catalog taxonomy calls on a slow local model need minutes, so they always timed out.

## Trigger

Both the 190-guide and 74-guide retopic runs died at the 120s mark. Root-cause analysis found the shared hard-coded timeout, not a logic bug, was the cause.

## Decision

Added call_model_blocking_with_timeout with an optional timeout parameter. Hot-path callers (triage, awareness, capture) keep the 120s default. Doctor's batch calls (taxonomy and merge via LlmClient::call) get 600s.

## Consequences

- Batch doctor jobs can run for minutes without dying; hot-path latency is unchanged
- Confirmed the fix: the 74-guide gemma run passed the 120s point where it previously died and completed successfully
- The timeout parameter is now a reusable contract — future batch callers can opt into longer timeouts without affecting the hot path

## Open Tail

*(none)*

## Evidence

- transcript lines 10529-10566

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-3-hard-coded-120s-model-timeout-parameterized.json`](transcripts/2026-05-29-3-hard-coded-120s-model-timeout-parameterized.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-3-hard-coded-120s-model-timeout-parameterized.json`](transcripts/raw/2026-05-29-3-hard-coded-120s-model-timeout-parameterized.json)
