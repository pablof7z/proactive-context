---
type: episode-card
date: 2026-05-28
session: 1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/1fe0f5c6-3cb7-46b8-aa15-3175c834dffb.jsonl
salience: root-cause
status: active
subjects:
  - rig-core
  - openrouter
  - max-tokens
  - cost-control
supersedes: []
related_claims: []
source_lines:
  - 3911-3918
  - 3920-3998
captured_at: 2026-06-29T10:47:07Z
---

# Episode: rig-core silently drops .max_tokens() on OpenRouter — route via additional_params

## Prior State

All rig agent builders (inject select+compile, generate main+decompose, capture) used `.max_tokens(N)` to cap LLM output, assuming OpenRouter forwarded the cap to the provider.

## Trigger

During v0.4 implementation, the engineer agent discovered the account balance was being exhausted because `rig-core`'s OpenRouter request struct silently drops `.max_tokens()` — output defaults to 64k tokens.

## Decision

Forward `max_tokens` via `.additional_params(serde_json::json!({"max_tokens": N}))` across all four rig agent builders (inject select + compile, generate main + decompose), mirroring the fix the v0.4 agent applied to capture.

## Consequences

- No rig agent builder in the system silently defaults to 64k output anymore; cost/runaway risk eliminated on inject and generate paths.
- The fix pattern is now established for any future rig builder addition.
- Inject's compile output stays short naturally so this was a latent cost bug, not a visible behavioral one.

## Open Tail

- The hardcoded `max_tokens: 2000` in capture could be made a config field (queued as nice-to-have by the v0.4 agent).

## Evidence

- transcript lines 3911-3918
- transcript lines 3920-3998

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-3-rig-core-silently-drops-max-tokens.json`](transcripts/2026-05-28-3-rig-core-silently-drops-max-tokens.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-3-rig-core-silently-drops-max-tokens.json`](transcripts/raw/2026-05-28-3-rig-core-silently-drops-max-tokens.json)
