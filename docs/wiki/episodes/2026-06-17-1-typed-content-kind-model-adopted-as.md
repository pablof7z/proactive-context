---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: architecture
status: superseded
subjects:
  - content-kind
  - typed-catalog
  - currentness
  - claim-status
  - backfill-taxonomy
supersedes:
  - 2026-06-17-1-content-taxonomy-made-first-class-flag
related_claims: []
source_lines:
  - 93-116
  - 192-203
  - 927-933
  - 997-1001
  - 1120-1140
captured_at: 2026-06-17T20:56:01Z
---

# Episode: Typed content-kind model adopted as first-class taxonomy

## Prior State

Wiki guides were flat untyped files with a topic domain grouping; the topic catalog started empty on each fresh session causing near-singleton fragmentation; research records (~100), episode cards (~159), and nouns were invisible to the selector — the system treated everything as an undifferentiated guide

## Trigger

Taxonomy cleanup directive; baseline audit showed 57 guides across 28 topics (~12 singletons), heavy historical/evidence corpus invisible to injection, and over-fragmented topics

## Decision

Adopt ContentKind / Currentness / Authority / ClaimStatus as central type model with flag-gated rollout (PC_TYPED_CATALOG, PC_SELECT_SOURCE_TYPES, PC_RESEARCH_CATALOG, PC_NOUN_CATALOG, PC_CLAIM_STATUS); add pc debug taxonomy audit, pc wiki backfill-taxonomy (idempotent), and TranscriptTurn canonical model — every flag defaults OFF and is byte-identical to baseline when off

## Consequences

- Baseline frozen: 75% guide recall, 71.4% user-direction recall, 2/10 stale-leak, 9/10 trajectory recoverable
- A1 (typed catalog) shown as clean win: recall 60→70%, stale-leak 1/10→0/10, trajectory 7/10→9/10
- A3 (research catalog) hurt recall on thin corpus (70→55%); remains off by default
- A4 vacuous (no nouns in store); claim catalog (Phase 5) still deferred
- Frozen labels reusable via --score-only for future validation runs

## Open Tail

- Confirm A1 with larger/repeat run before flipping PC_TYPED_CATALOG default-on
- Phase 5 claim catalog deferred pending reviewed cluster summaries

## Evidence

- transcript lines 93-116
- transcript lines 192-203
- transcript lines 927-933
- transcript lines 997-1001
- transcript lines 1120-1140

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-1-typed-content-kind-model-adopted-as.json`](transcripts/2026-06-17-1-typed-content-kind-model-adopted-as.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-1-typed-content-kind-model-adopted-as.json`](transcripts/raw/2026-06-17-1-typed-content-kind-model-adopted-as.json)
