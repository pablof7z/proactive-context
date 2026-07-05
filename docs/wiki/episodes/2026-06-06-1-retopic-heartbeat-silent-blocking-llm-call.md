---
type: episode-card
date: 2026-06-06
session: 6faa5ac2-c7f5-4c16-97bf-942f2c9b1098
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/6faa5ac2-c7f5-4c16-97bf-942f2c9b1098.jsonl
salience: product
status: active
subjects:
  - retopic-heartbeat
  - wiki-doctor
  - llm-call-ux
supersedes: []
related_claims: []
source_lines:
  - 159-219
captured_at: 2026-06-29T12:28:14Z
---

# Episode: Retopic heartbeat: silent blocking LLM call gets a live progress indicator

## Prior State

The `pc wiki doctor --retopic` command made a single blocking, non-streaming HTTP call to a cloud reasoning model (kimi-k2.6:cloud) with up to a 600s timeout and 3 retries. No output was produced between the initial 'asking … to propose a taxonomy' line and the final JSON result, making the tool appear frozen with no way to distinguish 'still thinking' from 'actually stuck'.

## Trigger

User reported that running --retopic 'takes ages and it doesn't do anything' and asked for insight into what's happening while it runs.

## Decision

Added a `with_heartbeat()` helper that spawns a side thread printing a braille spinner + elapsed-seconds counter to stderr while the blocking LLM call runs, then clears the line so it doesn't collide with the stdout taxonomy report. The retopic LLM call is now wrapped in this heartbeat.

## Consequences

- Users can now distinguish 'model still reasoning' from 'actually hung' via the ticking elapsed counter.
- Heartbeat output goes to stderr, preserving stdout for the machine-readable taxonomy report.
- If the counter climbs toward 600s, users know they're in the timeout window and a retry/abort is imminent.
- The same silent-blocking pattern exists in cluster MERGE/CONFIRM doctor calls but was not yet addressed.

## Open Tail

- Same heartbeat (or token streaming) could be added to the MERGE/CONFIRM LLM calls in doctor, which share the silent-blocking shape.
- Enabling `"stream": true` for the retopic call was proposed as an alternative to the elapsed timer but not adopted.

## Evidence

- transcript lines 159-219

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-retopic-heartbeat-silent-blocking-llm-call.json`](transcripts/2026-06-06-1-retopic-heartbeat-silent-blocking-llm-call.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-retopic-heartbeat-silent-blocking-llm-call.json`](transcripts/raw/2026-06-06-1-retopic-heartbeat-silent-blocking-llm-call.json)
