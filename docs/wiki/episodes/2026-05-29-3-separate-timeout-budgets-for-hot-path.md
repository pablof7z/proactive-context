---
type: episode-card
date: 2026-05-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
salience: architecture
status: active
subjects:
  - model-client
  - call-model-blocking
  - doctor-timeout
supersedes: []
related_claims: []
source_lines:
  - 10529-10567
captured_at: 2026-06-17T12:49:50Z
---

# Episode: Separate timeout budgets for hot-path vs batch model calls

## Prior State

call_model_blocking had a hard-coded 120s timeout shared by all callers (triage, awareness, capture open-question, doctor).

## Trigger

Retopic runs on local models died at 120s — whole-catalog taxonomy calls need minutes on slow hardware.

## Decision

Add call_model_blocking_with_timeout with configurable per-call timeout. Default remains 120s (hot-path: triage, awareness, capture). Doctor's batch calls (taxonomy, merge) get 600s.

## Consequences

- Batch doctor operations no longer time out on slow local models
- Hot-path latency guarantees unchanged
- Timeout is now a caller-specified parameter, not a global constant

## Open Tail

*(none)*

## Evidence

- transcript lines 10529-10567

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-3-separate-timeout-budgets-for-hot-path.json`](transcripts/2026-05-29-3-separate-timeout-budgets-for-hot-path.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-3-separate-timeout-budgets-for-hot-path.json`](transcripts/raw/2026-05-29-3-separate-timeout-budgets-for-hot-path.json)
