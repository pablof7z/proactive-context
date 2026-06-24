---
type: episode-card
date: 2026-06-06
session: 6e1a8676-e6b4-414c-b844-fbc3dbe437c0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/6e1a8676-e6b4-414c-b844-fbc3dbe437c0.jsonl
salience: root-cause
status: superseded
subjects:
  - capture-pipeline
  - mark-captured
  - timeout-retry
supersedes: []
related_claims: []
source_lines:
  - 253-315
captured_at: 2026-06-17T13:33:25Z
---

# Episode: mark_captured runs unconditionally on timeout/failure, blocking retry

## Prior State

The code comment (capture.rs:2362-2363) stated mark-after-loop was designed so that failed sessions could be retried; known-bugs.md recorded mark as running before the loop

## Trigger

Investigation of why 7 wiki.agent timeouts produced no wiki content revealed that the match block on agent_result catches Ok(Err) and Err(timeout) but does not return, so execution falls through to an unconditional mark_captured_in call — permanently marking timed-out sessions as captured

## Decision

Diagnosed as a bug: timed-out and API-errored sessions are marked captured despite the stated design intent, making plain re-runs skip them entirely. Recovery requires manually clearing capture markers (pc archeologist reset) before re-running with an increased timeout.

## Consequences

- The 7 richest sessions (longest/densest, most likely to hit the 300s cap) are silently dropped and excluded from future runs
- known-bugs.md entry was inaccurate (claimed mark ran before the loop; it runs after but unconditionally, same net effect)
- Fix options: make mark_captured conditional on success, or move it into the Ok(Ok) arm only

## Open Tail

- Whether to patch mark_captured to only execute on success, and whether to bump the 300s timeout for richer sessions

## Evidence

- transcript lines 253-315

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-mark-captured-runs-unconditionally-on-timeout.json`](transcripts/2026-06-06-1-mark-captured-runs-unconditionally-on-timeout.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-mark-captured-runs-unconditionally-on-timeout.json`](transcripts/raw/2026-06-06-1-mark-captured-runs-unconditionally-on-timeout.json)
