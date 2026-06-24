---
type: episode-card
date: 2026-06-09
session: 39fec889-adb7-4b6f-859f-2fb7a4ff3d97
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/39fec889-adb7-4b6f-859f-2fb7a4ff3d97.jsonl
salience: product
status: active
subjects:
  - pc-capture-sessionend-detach
supersedes: []
related_claims: []
source_lines:
  - 122-123
  - 206-208
  - 242-244
  - 292-296
  - 393-409
captured_at: 2026-06-17T13:53:44Z
---

# Episode: SessionEnd capture must return immediately, not block the harness

## Prior State

The SessionEnd `pc capture` hook ran `run_capture_from_input` synchronously in the foreground, blocking the harness from exiting for the full duration of the capture pipeline (LLM calls, routing, reconcile). Only the Stop hook (`pc capture --in <secs>`) forked into the background via the detached worker path. The SessionEnd wiring timeout was 120s, matching the blocking behavior.

## Trigger

User directive: "both should return immediately, otherwise `pc capture` prevents the harness from exiting for many seconds"

## Decision

SessionEnd `run_capture` now delegates to `run_capture_scheduled(0, harness)`, spawning the same detached `setsid` background worker the Stop hook uses, but with delay=0 (capture immediately, just not in the foreground). Hook wiring timeouts reduced from 120s to 10s for Claude and Hermes SessionEnd wirings. Diagnostic prefixes generalized from `capture --in:` to `capture:` since both paths now share the scheduled machinery.

## Consequences

- Foreground `pc capture` returns in ~44ms instead of blocking for seconds
- Detached worker still runs the full capture pipeline asynchronously; verified end-to-end via event log showing capture.start event 3s after foreground returned
- SessionEnd now coalesces with any in-flight Stop debounce worker (SIGTERMs the pending one); single-capture invariant preserved by existing `is_already_captured_in` marker dedup plus session lock (pre- and post-lock checks)
- Hook wiring timeout of 120s becomes dead config; aligned to 10s to signal intent (only affects new installs / re-installs of settings.json)

## Open Tail

*(none)*

## Evidence

- transcript lines 122-123
- transcript lines 206-208
- transcript lines 242-244
- transcript lines 292-296
- transcript lines 393-409

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-09-1-sessionend-capture-must-return-immediately-not.json`](transcripts/2026-06-09-1-sessionend-capture-must-return-immediately-not.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-09-1-sessionend-capture-must-return-immediately-not.json`](transcripts/raw/2026-06-09-1-sessionend-capture-must-return-immediately-not.json)
