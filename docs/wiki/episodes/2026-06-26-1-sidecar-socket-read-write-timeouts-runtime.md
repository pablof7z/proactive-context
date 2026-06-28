---
type: episode-card
date: 2026-06-26
session: 151a7e32-2bff-4b31-9196-dd6060fdb411
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/151a7e32-2bff-4b31-9196-dd6060fdb411.jsonl
salience: root-cause
status: active
subjects:
  - sidecar-hang
  - client-timeout
  - runtime-shutdown
supersedes: []
related_claims: []
source_lines:
  - 545-751
captured_at: 2026-06-26T07:32:22Z
---

# Episode: Sidecar socket read/write timeouts + runtime shutdown hardening

## Prior State

Sidecar clients block indefinitely on socket reads; Runtime::drop() waits indefinitely for uncancellable spawn_blocking tasks (per Tokio 1.52.3 semantics).

## Trigger

Theoretical analysis of blocking calls in claude_sidecar.rs and embed_sidecar.rs identified indefinite-block risk; codex confirmed via Tokio docs. Although not the cause of this session's actual CPU issue (daemon embedding churn), the bugs are real and latent.

## Decision

Add 10-second socket read/write timeouts in both sidecar clients (claude_sidecar.rs, embed_sidecar.rs); call rt.shutdown_background() after each block_on in inject.rs and capture.rs to detach pending tasks before runtime drop.

## Consequences

- Sidecar communication is now timeout-bounded; a wedged sidecar cannot pin the entire process indefinitely.
- Runtime drop is no longer a hang-point; background tasks are detached before shutdown.
- Inject and capture processes now exit promptly even if background work is incomplete.
- All 352 unit tests pass; code compiles cleanly and installs.

## Open Tail

*(none)*

## Evidence

- transcript lines 545-751

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-sidecar-socket-read-write-timeouts-runtime.json`](transcripts/2026-06-26-1-sidecar-socket-read-write-timeouts-runtime.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-sidecar-socket-read-write-timeouts-runtime.json`](transcripts/raw/2026-06-26-1-sidecar-socket-read-write-timeouts-runtime.json)
