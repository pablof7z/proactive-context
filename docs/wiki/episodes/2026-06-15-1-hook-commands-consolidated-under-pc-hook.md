---
type: episode-card
date: 2026-06-15
session: 64c94ab4-45c5-4746-9d50-678dcfa6851c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/64c94ab4-45c5-4746-9d50-678dcfa6851c.jsonl
salience: reversal
status: active
subjects:
  - cli-hook-subcommand
  - harness-wiring
supersedes: []
related_claims: []
source_lines:
  - 1-3
  - 276-283
  - 386-389
  - 442-458
captured_at: 2026-06-17T14:13:50Z
---

# Episode: Hook commands consolidated under `pc hook` subcommand

## Prior State

Hook-invoked commands were flat top-level CLI commands (`pc capture`, `pc inject`, `pc session-start`, `pc statusline`). Harness wiring used bare args like `"inject"`, `"capture"`. Awareness/PostToolUse wiring still present in harness registries.

## Trigger

User directive to mirror tenex-edge's `hook` subcommand pattern with `--harness <claude|opencode|codex>`, followed by explicit rejection of backward compatibility ("no backwards compatibility ffs") when assistant initially preserved old top-level commands.

## Decision

All harness-hook commands are now nested under `pc hook <subcommand>` (inject, capture, session-start, statusline) with a `--harness` flag. Old top-level commands were entirely removed. Awareness PostToolUse wiring was dropped from all harness registries.

## Consequences

- Existing installations must be re-wired via `pc install`; old `pc capture` / `pc inject` invocations will fail
- CLI surface is now cleanly namespaced: hook-driven lifecycle under `pc hook`, everything else stays top-level
- Awareness feature fully removed — no residual wiring paths remain
- Wiring args changed from bare (`"inject"`) to namespaced (`"hook inject"`)

## Open Tail

*(none)*

## Evidence

- transcript lines 1-3
- transcript lines 276-283
- transcript lines 386-389
- transcript lines 442-458

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-15-1-hook-commands-consolidated-under-pc-hook.json`](transcripts/2026-06-15-1-hook-commands-consolidated-under-pc-hook.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-15-1-hook-commands-consolidated-under-pc-hook.json`](transcripts/raw/2026-06-15-1-hook-commands-consolidated-under-pc-hook.json)
