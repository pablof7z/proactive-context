---
type: episode-card
date: 2026-06-06
session: 6faa5ac2-c7f5-4c16-97bf-942f2c9b1098
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/6faa5ac2-c7f5-4c16-97bf-942f2c9b1098.jsonl
salience: product
status: active
subjects:
  - retopic-heartbeat
  - doctor-progress-feedback
supersedes: []
related_claims: []
source_lines:
  - 1-8
  - 159-218
captured_at: 2026-06-17T13:28:20Z
---

# Episode: retopic heartbeat progress indicator

## Prior State

The `--retopic` command made a single blocking non-streaming LLM call with zero stdout/stderr output until the full JSON response returned, making it indistinguishable from a hang for up to 600s (with 3 retries).

## Trigger

User reported the command 'takes ages and doesn't do anything' and asked for visibility into what it was doing.

## Decision

Added a `with_heartbeat()` helper that spawns a side thread printing a braille spinner + elapsed seconds to stderr while the blocking call runs, then clears its line so it doesn't collide with the stdout taxonomy report. The retopic LLM call is now wrapped in this heartbeat.

## Consequences

- Users can now distinguish 'still thinking' from 'actually stuck' during retopic runs
- Elapsed counter gives a sense of progress toward the 600s timeout window
- The same silent-blocking pattern exists in MERGE/CONFIRM doctor calls — heartbeat not yet applied there
- Root cause confirmed: latency is model-side reasoning (kimi-k2.6 chain-of-thought), not payload size (~1,700 tokens in, ~600 out)

## Open Tail

- Apply heartbeat to cluster MERGE/CONFIRM calls in doctor (same silent-blocking shape)
- Consider switching retopic to streaming (`"stream": true`) for token-by-token visibility instead of just an elapsed timer
- Consider using a lighter non-reasoning model for the taxonomy bucketing task

## Evidence

- transcript lines 1-8
- transcript lines 159-218

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-retopic-heartbeat-progress-indicator.json`](transcripts/2026-06-06-1-retopic-heartbeat-progress-indicator.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-retopic-heartbeat-progress-indicator.json`](transcripts/raw/2026-06-06-1-retopic-heartbeat-progress-indicator.json)
