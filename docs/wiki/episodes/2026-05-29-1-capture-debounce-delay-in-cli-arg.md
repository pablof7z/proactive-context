---
type: episode-card
date: 2026-05-29
session: fbd3d6f8-1b55-4271-aaf4-de0790b5120b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/fbd3d6f8-1b55-4271-aaf4-de0790b5120b.jsonl
salience: architecture
status: active
subjects:
  - capture-debounce
  - cli-arg-semantics
  - pending-capture
supersedes: []
related_claims: []
source_lines:
  - 274-297
  - 456-460
  - 644-676
captured_at: 2026-06-29T11:45:32Z
---

# Episode: Capture debounce delay: --in CLI arg promoted from dead flag to config-overriding source of truth

## Prior State

The Stop hook called `pc capture --in 300`, but the `300` was dead code — run_capture_scheduled accepted delay_secs yet never used it for timing. The actual delay came solely from capture_debounce_secs in config. The --in number only appeared in a log message that actively lied if it diverged from config. Two redundant 300s values created a footgun where changing the hook number did nothing.

## Trigger

User asked why both --in 300 and capture_debounce_secs: 300 exist. Root-cause investigation revealed delay_secs in run_capture_scheduled was referenced only in its signature and a misleading log line; the deferred runner re-read config independently, ignoring the CLI value entirely.

## Decision

Adopted Option 1: --in is now config-default with an optional override. CLI arg changed from Option<u64> to Option<Option<u64>> with num_args=0..=1, so bare --in uses capture_debounce_secs from config and --in 90 overrides to 90s. The resolved delay is stored in PendingCapture.debounce_secs and the deferred runner sleeps on that stored value instead of re-reading config. Settings.json hook changed from `pc capture --in 300` to bare `pc capture --in`, making capture_debounce_secs the single source of truth.

## Consequences

- capture_debounce_secs in config is now the sole source of truth for debounce delay; the CLI --in value actually drives timing instead of being ignored
- The deferred runner no longer loads config at all — it reads debounce_secs from the pending capture file
- The delay={}s log line is now truthful — it reflects the actual sleep duration
- default_capture_debounce_secs was made pub for reuse as the serde default, preserving backward-compat with old pending files
- Bare --in, --in 60 (space), and --in=60 (equals) all parse correctly per empirical testing against the installed binary
- The old binary at ~/.bin/pc is a copy not a symlink, so cargo build alone doesn't deploy — just install is required for hook changes to take effect

## Open Tail

- The actual delay value is still 300s (5 min), unchanged — the user's original goal of 1-minute capture was deferred in favor of the refactor; user has not yet confirmed setting capture_debounce_secs to 60
- Code changes are uncommitted; settings.json edit lives in global config outside the repo

## Evidence

- transcript lines 274-297
- transcript lines 456-460
- transcript lines 644-676

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-capture-debounce-delay-in-cli-arg.json`](transcripts/2026-05-29-1-capture-debounce-delay-in-cli-arg.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-capture-debounce-delay-in-cli-arg.json`](transcripts/raw/2026-05-29-1-capture-debounce-delay-in-cli-arg.json)
