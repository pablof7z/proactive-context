---
type: episode-card
date: 2026-06-18
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: product
status: superseded
subjects:
  - model-selection
  - source-type-compilation
  - eval-arms
supersedes: []
related_claims: []
source_lines:
  - 3032-3038
  - 3096-3112
captured_at: 2026-06-18T08:44:25Z
---

# Episode: A2 higher-power model shipped as default for source-type compilation

## Prior State

A0 was the default compilation arm; source-type SELECT/COMPILE had no differentiated model power

## Trigger

Converging eval evidence: high-power K=3 judge eval and independent deterministic token-overlap cross-check both confirmed A0<A1<A2 ordering with zero stale-leak across all arms and cost within ~15% budget

## Decision

A2 (higher-power model) shipped as default-on for source-type compilation (commit ef678dc), removing the feature flag gate — gates abandoned per session directive

## Consequences

- Better injection quality and currentness at ~15% cost increase
- Zero stale-leak confirmed across A0/A1/A2 arms
- 309 tests pass with A2 default-on
- Deterministic cross-check methodology established as validation partner for judge evals

## Open Tail

- Cost delta (~15%) may need monitoring at scale
- A2 is now the production default with no fallback gate

## Evidence

- transcript lines 3032-3038
- transcript lines 3096-3112

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-a2-higher-power-model-shipped-as.json`](transcripts/2026-06-18-2-a2-higher-power-model-shipped-as.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-a2-higher-power-model-shipped-as.json`](transcripts/raw/2026-06-18-2-a2-higher-power-model-shipped-as.json)
