---
type: episode-card
date: 2026-06-17
session: 8eff6130-2e37-410c-968c-a73ff4acc88c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8eff6130-2e37-410c-968c-a73ff4acc88c.jsonl
salience: architecture
status: superseded
subjects:
  - content-kind-enum
  - typed-catalog
  - source-type-select
  - taxonomy-backfill
supersedes:
  - 2026-06-17-1-content-taxonomy-made-first-class-with
related_claims: []
source_lines:
  - 93-117
  - 599-620
  - 660-680
  - 749-775
  - 1253-1280
  - 1383-1418
captured_at: 2026-06-17T18:15:38Z
---

# Episode: Content taxonomy becomes first-class typed system

## Prior State

Guides are flat wiki files with a `topic` domain grouping; episode cards, research records, and noun entries are largely injection-invisible. The selector catalog is untyped (slug, title, summary, score only). 57 guides across 28 topics (~12 singletons) with 100 research records and 159 episode cards unseen by the injector.

## Trigger

Taxonomy implementation plan directive; Phase 0 audit confirmed over-fragmentation and injection-invisibility of the historical/evidence corpus.

## Decision

Introduced a central ContentKind/Currentness/Authority type model (content_kind.rs). CatalogItem gains kind + currentness fields. Research and noun source rows added behind PC_TYPED_CATALOG / PC_RESEARCH_CATALOG / PC_NOUN_CATALOG flags. Source-type SELECT guidance added behind PC_SELECT_SOURCE_TYPES. Idempotent `pc wiki backfill-taxonomy` command. All new flags default OFF and are byte-identical to baseline when off.

## Consequences

- Typed catalog enables type-aware injection when flags are flipped
- Backfill is idempotent and non-destructive (verified byte-identical on rerun)
- Baseline probe metrics frozen: guide restatement recall 75%, user-direction recall 71.4%, stale-current leak 2/10, trajectory recoverable 9/10, latency p50/p95 5.0s/12.7s
- Frozen labels reusable via --score-only for cheap future arms
- docs/wiki/ discovered to be untracked/generated state — concurrent peer sessions can regenerate it mid-run (57→3 guides observed)

## Open Tail

- A0–A5 eval arms deferred (six full eval runs; gate for flipping any default on)
- Phase 5 (claim catalog rows) deferred pending cluster-summary review
- docs/wiki/ volatility hazard unmitigated — needs source-of-truth decision

## Evidence

- transcript lines 93-117
- transcript lines 599-620
- transcript lines 660-680
- transcript lines 749-775
- transcript lines 1253-1280
- transcript lines 1383-1418

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-1-content-taxonomy-becomes-first-class-typed.json`](transcripts/2026-06-17-1-content-taxonomy-becomes-first-class-typed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-1-content-taxonomy-becomes-first-class-typed.json`](transcripts/raw/2026-06-17-1-content-taxonomy-becomes-first-class-typed.json)
