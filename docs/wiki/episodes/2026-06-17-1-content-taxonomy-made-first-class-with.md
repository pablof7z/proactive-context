---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: architecture
status: superseded
subjects:
  - content-kind-type-model
  - typed-catalog
  - claim-status
  - source-type-select
  - taxonomy-backfill
supersedes: []
related_claims: []
source_lines:
  - 93-117
  - 349-352
  - 599-619
  - 750-795
  - 891-934
  - 1116-1141
captured_at: 2026-06-17T13:28:48Z
---

# Episode: Content taxonomy made first-class with feature-flag rollout

## Prior State

Content types existed implicitly — guides as flat wiki files with `topic` metadata, claims in append-only JSONL, episodes in a subdirectory, research records and nouns as separate subsystems. The injection catalog (`CatalogItem`) treated everything as undifferentiated rows with no kind or currentness information. No unified type model existed across capture, storage, and injection.

## Trigger

The content taxonomy implementation plan prescribed making the existing taxonomy first-class and operational: type-aware project memory where guides answer what is true now, claims preserve atomic facts, episode cards explain direction changes, research records preserve investigation evidence, and nouns ground project-specific entities.

## Decision

Implemented a central `ContentKind` type model (CurrentGuide, EpisodeCard, ResearchRecord, NounEntry, Claim, CommittedMarkdown with Currentness levels and Authority tags) as `content_kind.rs`. Extended `CatalogItem` with kind/currentness fields behind `PC_TYPED_CATALOG`/`PC_RESEARCH_CATALOG`/`PC_NOUN_CATALOG` flags. Added `ClaimStatus{Settled,Proposed,Unknown}` to `ClaimRecord` behind `PC_CLAIM_STATUS`. Extended SELECT preamble with source-type guidance behind `PC_SELECT_SOURCE_TYPES`. Added `pc wiki backfill-taxonomy` for idempotent typed index generation. All flags default OFF — byte-identical to baseline when disabled.

## Consequences

- Baseline probe metrics frozen for future comparison: guide restatement recall 75%, user-direction recall 71.4%, stale-current leak 2/10, trajectory recoverable 9/10, latency p50/p95 5.0s/12.7s
- Phase 5 (claim catalog rows) deferred per plan rule: don't add claim: rows until cluster summaries have stable currentness and pass review
- Phase 3 A0–A5 eval arms deferred — six full eval runs gated on deciding to flip a default on
- docs/wiki/ discovered to be untracked/generated state; a concurrent peer session regenerated it mid-run (57→3 guides), confirming multi-agent shared-state hazard for wiki-dependent workflows

## Open Tail

- A0–A5 eval arms needed before any feature flag can default ON
- Phase 5 (claim catalog) blocked on reviewed cluster summaries

## Evidence

- transcript lines 93-117
- transcript lines 349-352
- transcript lines 599-619
- transcript lines 750-795
- transcript lines 891-934
- transcript lines 1116-1141

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-1-content-taxonomy-made-first-class-with.json`](transcripts/2026-06-17-1-content-taxonomy-made-first-class-with.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-1-content-taxonomy-made-first-class-with.json`](transcripts/raw/2026-06-17-1-content-taxonomy-made-first-class-with.json)
