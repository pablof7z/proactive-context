---
type: episode-card
date: 2026-06-18
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: product
status: active
subjects:
  - inject-strategy
  - eval-a2
  - default-behavior
supersedes:
  - 2026-06-18-2-a2-higher-power-model-shipped-as
related_claims: []
source_lines:
  - 3057-3058
  - 3074-3075
captured_at: 2026-06-18T08:51:23Z
---

# Episode: A2 injection strategy promoted to default-on

## Prior State

A0 was the baseline/default injection strategy; A1 and A2 were eval arms under test, gated behind feature flags.

## Trigger

Converging evidence from high-power K=3 judge eval and independent deterministic token-overlap cross-check both confirmed A0 < A1 < A2 quality ordering, with zero stale-leak across all arms and cost within 15% budget.

## Decision

A2 promoted to default-on (shipped in commit ef678dc), removing the feature flag gate. No gate applied — bold call on converging evidence.

## Consequences

- All users now receive A2 injection quality by default, changing visible prompt-injection behavior
- 309 tests pass with A2 as default
- Sets precedent for eval-driven default promotion without a separate gate-review step

## Open Tail

*(none)*

## Evidence

- transcript lines 3057-3058
- transcript lines 3074-3075

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-a2-injection-strategy-promoted-to-default.json`](transcripts/2026-06-18-2-a2-injection-strategy-promoted-to-default.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-a2-injection-strategy-promoted-to-default.json`](transcripts/raw/2026-06-18-2-a2-injection-strategy-promoted-to-default.json)
