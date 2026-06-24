---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: architecture
status: superseded
subjects:
  - content-taxonomy
  - typed-catalog
  - content-kind-model
  - claim-status
supersedes:
  - 2026-06-17-1-content-taxonomy-becomes-first-class-typed
  - 2026-06-17-2-claim-status-persisted-on-claimrecord
related_claims: []
source_lines:
  - 81-117
  - 815-821
  - 927-934
  - 1076-1084
  - 1276-1316
  - 1383-1418
captured_at: 2026-06-17T19:42:51Z
---

# Episode: Content taxonomy made first-class, flag-gated, with frozen baseline

## Prior State

Taxonomy types (guides, claims, episode cards, research records, nouns) existed informally as flat wiki files with a `topic` domain grouping. The selector/catalog treated all content as undifferentiated wiki entries. Research records (~100) and nouns were invisible to the injection pipeline. Near-singleton topic fragmentation (57 guides across 28 topics, ~12 singletons) resulted from an empty topic catalog on fresh wikis. Claims had no lifecycle state.

## Trigger

Directive to execute the content-taxonomy implementation plan; inventory audit (`pc debug taxonomy`) revealing that 100 research records, 159 episode cards, and fragmented guides were largely invisible to the selector.

## Decision

Implemented a full type-aware taxonomy layer: (1) `ContentKind`/`Currentness`/`Authority`/`ClaimOp`/`GuideKind` model in `content_kind.rs`; (2) typed `CatalogItem` with kind/currentness fields, `research:`/`noun:` source rows; (3) `ClaimStatus{Settled,Proposed,Unknown}` on `ClaimRecord` with serde default for backward compat; (4) source-type SELECT guidance; (5) canonical `TranscriptTurn`; (6) idempotent `pc wiki backfill-taxonomy`. All behavior changes gated behind feature flags (`PC_TYPED_CATALOG`, `PC_RESEARCH_CATALOG`, `PC_NOUN_CATALOG`, `PC_SELECT_SOURCE_TYPES`, `PC_CLAIM_STATUS`) defaulting OFF — byte-identical to baseline when off. Phase 5 (claim catalog rows) explicitly deferred pending cluster-summary review.

## Consequences

- Research records and nouns can now appear as typed catalog rows when their respective flags are enabled.
- Claims carry lifecycle state; `PC_CLAIM_STATUS` can keep proposed ideas out of current-guide prose.
- Selector can receive type-aware guidance about what each source kind contains.
- Baseline metrics frozen: guide restatement recall 75%, user-direction recall 71.4%, stale-current leak 2/10, trajectory X→Y recoverable 9/10, latency p50/p95 5.0s/12.7s.
- Eval harness auto-overwrites `claims-first-validation-results.md` (clobbered ~1475 lines of Runs 11–12 history) — flagged as a real bug to fix.
- `docs/wiki/` is untracked/generated; concurrent peer sessions can regenerate it mid-run (57→3 guides observed).

## Open Tail

- Phase 5 (claim catalog) deferred pending reviewed cluster summaries.
- A0–A4 eval arms deferred — need decision to flip any flag default before running expensive validation.
- No nouns in the eval store, so noun-catalog validation will be vacuous until noun capture matures.

## Evidence

- transcript lines 81-117
- transcript lines 815-821
- transcript lines 927-934
- transcript lines 1076-1084
- transcript lines 1276-1316
- transcript lines 1383-1418

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-1-content-taxonomy-made-first-class-flag.json`](transcripts/2026-06-17-1-content-taxonomy-made-first-class-flag.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-1-content-taxonomy-made-first-class-flag.json`](transcripts/raw/2026-06-17-1-content-taxonomy-made-first-class-flag.json)
