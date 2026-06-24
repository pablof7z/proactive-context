---
type: episode-card
date: 2026-06-18
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: architecture
status: active
subjects:
  - content-taxonomy
  - typed-compilation
  - source-types
supersedes:
  - 2026-06-17-1-content-taxonomy-made-first-class-via
  - 2026-06-18-1-typed-catalog-source-type-select-shipped
related_claims: []
source_lines:
  - 77-81
  - 93-116
  - 192-203
captured_at: 2026-06-18T08:44:25Z
---

# Episode: Content taxonomy promoted to first-class operational model

## Prior State

Taxonomy existed but was uneven — guides were flat wiki files with topic domain grouping, claims/episodes/research/nouns had inconsistent support, and per-session ROUTE mints near-singleton topics because the catalog starts empty on a fresh wiki

## Trigger

Explicit directive to execute the content-taxonomy implementation plan (Plans/content-taxonomy-implementation-experiment-plan.md), making capture taxonomy first-class and operational with type-aware project memory

## Decision

Adopted a typed content model where guides, claims, episodes, research, and nouns are first-class content types with type-aware SELECT/COMPILE semantics; feature-flagged rollout through Phases 0–4, preserving current guide/topic semantics during migration

## Consequences

- Phases 0 (baseline/audit) through Phase 3 (typed SELECT/COMPILE with eval arms A0–A5) implemented
- A2 (higher-power model) emerged as the winning eval arm and was shipped default-on
- Phase 5 claim catalog work identified as next step
- Wiki regeneration for 3 of 4 projects completed via archeologist (hl: 40 guides, tenex-edge: 84 guides, podcast-player: 98 guides); NMP parked for remote execution
- 309 tests passing at session end
- Near-singleton topic proliferation root cause (empty catalog on fresh wiki) addressed by typed consolidation

## Open Tail

- NMP wiki regeneration still pending (transferred to remote machine)
- Phase 5 claim catalog not yet implemented
- Phases 5–7 (claim catalog, typed transcript substrate, migration/backfill) remain

## Evidence

- transcript lines 77-81
- transcript lines 93-116
- transcript lines 192-203

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-content-taxonomy-promoted-to-first-class.json`](transcripts/2026-06-18-1-content-taxonomy-promoted-to-first-class.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-content-taxonomy-promoted-to-first-class.json`](transcripts/raw/2026-06-18-1-content-taxonomy-promoted-to-first-class.json)
