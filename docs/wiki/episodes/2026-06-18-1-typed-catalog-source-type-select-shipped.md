---
type: episode-card
date: 2026-06-18
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: product
status: superseded
subjects:
  - inject-pipeline
  - content-kind
  - taxonomy-flags
  - pc-typed-catalog
  - pc-select-source-types
supersedes: []
related_claims: []
source_lines:
  - 93-117
  - 2424-2441
  - 2587-2589
  - 2628-2631
  - 2749-2750
captured_at: 2026-06-18T07:12:23Z
---

# Episode: Typed catalog + source-type SELECT shipped default-on

## Prior State

Content types existed in the capture pipeline but were not surfaced at inject time. PC_TYPED_CATALOG and PC_SELECT_SOURCE_TYPES were default-off feature flags; the catalog and SELECT preamble were byte-identical to the pre-taxonomy baseline when flags were off.

## Trigger

High-power eval (K=3 judge majority + paired bootstrap confidence interval + independent deterministic token-overlap cross-check) converged on A0 < A1 < A2 with zero stale-leak across all arms and cost within 15% budget — no ambiguous result requiring a coward's gate.

## Decision

Flip PC_TYPED_CATALOG and PC_SELECT_SOURCE_TYPES to default-ON (shipped in commit ef678dc). A new taxonomy_flag_default_on helper replaces taxonomy_flag for both, so they opt-out via PC_*=0 rather than opt-in. Catalog lines now append compact [kind-label] type hints; SELECT preamble now includes source-type guidance — both active without any user configuration.

## Consequences

- Baseline-identity tests required explicit PC_*=0 to preserve byte-identical comparison against pre-taxonomy output (309 tests updated and passing).
- K=3 judge + paired bootstrap CI established as the eval standard for future flag-ship decisions.
- Four project wikis (hl, tenex-edge, podcast-player, nostr-multi-platform) were regenerated from scratch on the new build to validate production behavior; 3 completed, 1 parked partial.
- Both flags remain reversible: PC_TYPED_CATALOG=0 and PC_SELECT_SOURCE_TYPES=0 restore pre-taxonomy behavior.
- Audit/reporting tooling (taxonomy_report) updated to reflect default-on status with ship date.

## Open Tail

- nostr-multi-platform wiki regen incomplete — parked at ~104/789 sessions (~68 guides written); must resume (not restart) on the machine holding ~/.claude/projects transcripts.
- Phase 5 (claim catalog) and a higher-power eval to tighten CIs remain deferred.
- eval harness bug where pc eval overwrites claims-first-validation-results.md still unfixed.

## Evidence

- transcript lines 93-117
- transcript lines 2424-2441
- transcript lines 2587-2589
- transcript lines 2628-2631
- transcript lines 2749-2750

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-typed-catalog-source-type-select-shipped.json`](transcripts/2026-06-18-1-typed-catalog-source-type-select-shipped.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-typed-catalog-source-type-select-shipped.json`](transcripts/raw/2026-06-18-1-typed-catalog-source-type-select-shipped.json)
