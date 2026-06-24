---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: architecture
status: superseded
subjects:
  - content-kind-model
  - typed-catalog
  - claim-status
supersedes:
  - 2026-06-17-1-typed-content-kind-model-adopted-as
related_claims: []
source_lines:
  - 93-117
  - 1376-1380
  - 1389-1398
captured_at: 2026-06-17T22:04:35Z
---

# Episode: Content taxonomy made first-class via typed content-kind model

## Prior State

Wiki guides were flat files with guide metadata and a `topic` domain grouping; the per-session ROUTE step minted near-singleton topics because the catalog started empty. 100 research records + 159 episode cards (66 active) were largely invisible to the selector; 57 guides across 28 topics with ~12 singletons.

## Trigger

User directive to execute the full content taxonomy plan; inventory audit confirmed heavy historical/evidence corpus was invisible to injection and guides were over-fragmented.

## Decision

Introduced a central `ContentKind`/`Currentness`/`Authority`/`ClaimOp` type model (`src/content_kind.rs`), a typed selector catalog (`PC_TYPED_CATALOG`), research/noun source rows (`PC_RESEARCH_CATALOG`/`PC_NOUN_CATALOG`), `ClaimStatus{Settled,Proposed,Unknown}` on claims (`PC_CLAIM_STATUS`), canonical `TranscriptTurn` model, and an idempotent `pc wiki backfill-taxonomy` command. All flag-gated, default-OFF, byte-identical to baseline when disabled.

## Consequences

- 302 tests green across 11 commits on `taxonomy-work` branch; every new feature is inert until flags are flipped.
- Baseline measured: 75% guide recall, 71.4% user-direction recall, 2/10 stale-current leak, 9/10 trajectory X→Y recoverable.
- Typed catalog (A1 arm) showed +10 recall on first run but didn't replicate on second run — default-on decision deferred pending higher-power eval.
- Phase 5 (claim catalog) deferred pending reviewed cluster summaries.

## Open Tail

- Default-on decision for PC_TYPED_CATALOG awaits high-power K=3 eval results.
- Phase 5 claim catalog still deferred.

## Evidence

- transcript lines 93-117
- transcript lines 1376-1380
- transcript lines 1389-1398

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-1-content-taxonomy-made-first-class-via.json`](transcripts/2026-06-17-1-content-taxonomy-made-first-class-via.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-1-content-taxonomy-made-first-class-via.json`](transcripts/raw/2026-06-17-1-content-taxonomy-made-first-class-via.json)
