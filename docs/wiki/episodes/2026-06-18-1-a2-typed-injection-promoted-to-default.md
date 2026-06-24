---
type: episode-card
date: 2026-06-18
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: reversal
status: active
subjects:
  - inject-taxonomy
  - source-type-select
  - eval-arms
supersedes: []
related_claims: []
source_lines:
  - 93-117
  - 3118-3126
captured_at: 2026-06-18T14:10:48Z
---

# Episode: A2 typed injection promoted to default-on, bypassing planned feature-flag gate

## Prior State

The content taxonomy implementation plan explicitly called for behavior-changing work to stay behind feature flags during phased rollout (Phase 3: 'flagged, eval arms A0-A5'). The cautious default was flag-gated rollout with A0 (baseline) as the safe default.

## Trigger

Converging eval evidence from high-power K=3 judge eval plus independent deterministic token-overlap cross-check showed A0<A1<A2 ordering, zero stale-leak across all arms, and cost within 15% budget — eliminating the risk justification for the flag gate.

## Decision

Ship A2 (typed injection) as default-on, removing the feature-flag gate the plan prescribed. The plan's phased gate approach was replaced by direct default-on based on sufficient empirical evidence.

## Consequences

- Commit ef678dc shipped A2 default-on with 309 tests passing
- All subsequent wiki regenerations (hl, tenex-edge, podcast-player) ran under the new A2 semantics
- The flag-gate mechanism still exists in code but is not activated — future rollback is possible but not the default path
- Phase 4+ of the plan (eval arms A0-A5 with flags) is effectively collapsed into the A2 default-on decision

## Open Tail

- NMP wiki regeneration still pending (transferred to remote machine for continuation)
- Longitudinal eval not yet measured — A2 default-on in production across real projects may surface issues the controlled eval did not

## Evidence

- transcript lines 93-117
- transcript lines 3118-3126

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-a2-typed-injection-promoted-to-default.json`](transcripts/2026-06-18-1-a2-typed-injection-promoted-to-default.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-a2-typed-injection-promoted-to-default.json`](transcripts/raw/2026-06-18-1-a2-typed-injection-promoted-to-default.json)
