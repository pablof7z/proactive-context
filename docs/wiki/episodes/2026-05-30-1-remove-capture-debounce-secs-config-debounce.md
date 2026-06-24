---
type: episode-card
date: 2026-05-30
session: bb497722-f876-466c-89df-68647eca0e4b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/bb497722-f876-466c-89df-68647eca0e4b.jsonl
salience: architecture
status: active
subjects:
  - capture-debounce
  - stop-hook
  - config
supersedes:
  - 2026-05-29-1-capture-in-flag-made-functional-debounce
related_claims: []
source_lines:
  - 55-338
captured_at: 2026-06-17T13:10:28Z
---

# Episode: Remove capture_debounce_secs config — debounce delay now comes only from --in argument

## Prior State

The capture debounce delay had a dual-source: `capture_debounce_secs` in config (default 300s) was the fallback when the Stop hook ran `pc capture --in` without a value; `--in <SECS>` could override it. The Stop hook in settings.json passed bare `--in` with no value, relying on the config default.

## Trigger

User directive (line 55): 'the capture shouldn't have a capture_debounce_secs config -- it should ONLY use whatever --in provided'

## Decision

Eliminated `capture_debounce_secs` from the config struct entirely. Made `--in` require an explicit value (`--in 300`). `run_capture_scheduled` now takes `u64` directly instead of `Option<u64>`. The Stop hook in settings.json now passes `--in 300` explicitly, making the debounce duration an argument concern rather than a config concern.

## Consequences

- Debounce delay is no longer user-configurable via config file; it is fixed at the value passed in the Stop hook command (300s)
- Bare `pc capture --in` (no value) is no longer valid CLI usage
- PendingCapture.debounce_secs no longer has a serde default fallback
- Default config struct no longer includes capture_debounce_secs field
- Changing the debounce window now requires editing the Stop hook command in settings.json rather than a config value

## Open Tail

*(none)*

## Evidence

- transcript lines 55-338

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-30-1-remove-capture-debounce-secs-config-debounce.json`](transcripts/2026-05-30-1-remove-capture-debounce-secs-config-debounce.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-30-1-remove-capture-debounce-secs-config-debounce.json`](transcripts/raw/2026-05-30-1-remove-capture-debounce-secs-config-debounce.json)
