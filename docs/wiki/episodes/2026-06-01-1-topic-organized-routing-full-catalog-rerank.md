---
type: episode-card
date: 2026-06-01
session: 8fa18555-86b6-492d-9b13-1865774df99c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8fa18555-86b6-492d-9b13-1865774df99c.jsonl
salience: architecture
status: active
subjects:
  - capture-route
  - wiki-index
  - guide-frontmatter
  - route-decision
supersedes: []
related_claims: []
source_lines:
  - 1211-1250
  - 1250-1253
  - 1632-1645
  - 2010-2039
captured_at: 2026-06-17T13:13:07Z
---

# Episode: Topic-organized routing: full-catalog RERANK + topic field on guides

## Prior State

Wiki guides lived in a flat layout at docs/wiki/<slug>.md with no topic metadata. RERANK saw only ~8 cosine-recalled candidates per claim, causing it to mint near-duplicate guides for topics it couldn't see. _index.md was a single flat table with no grouping. Phase 1 (altitude fix, shipped earlier) raised ROUTE granularity but did not add topics.

## Trigger

User noticed no topics were generated after running archeologist ('what happened to the topics?'), learned Phase 2 was unbuilt, and directed implementation with 'do it'.

## Decision

Implemented Phase 2 topic-organized routing: (1) added `topic: String` to GuideFrontmatter and IndexRow, parsed from frontmatter and serialized when non-empty; (2) RERANK now receives the full existing wiki catalog grouped by topic before per-claim candidates, so it can route claims to existing topic areas instead of minting blind; (3) RouteDecision struct gains a `topic` field in the LLM output schema; (4) `_index.md` renders topic-grouped sections with a machine-parseable 7-column table; (5) existing guides get topic back-filled on their next RECONCILE update.

## Consequences

- RERANK has global visibility into all existing guides, reducing near-duplicate slug creation for the same topic
- Topic is optional/empty on legacy guides, filled incrementally as captures update them — backward-compatible
- All 73 unit tests pass; 4 files changed (+104/−25 lines)
- Future Phase: physical topics/ directory layout was considered but deferred in favor of metadata-only to avoid touching 10+ call sites

## Open Tail

- Staleness retirement (Phase 3: demote-not-delete for guides with absence-of-signal) remains unbuilt
- Physical topics/ directory vs metadata-only still an open design choice for later
- Need validation run against a real project to confirm topic grouping reduces guide fragmentation

## Evidence

- transcript lines 1211-1250
- transcript lines 1250-1253
- transcript lines 1632-1645
- transcript lines 2010-2039

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-01-1-topic-organized-routing-full-catalog-rerank.json`](transcripts/2026-06-01-1-topic-organized-routing-full-catalog-rerank.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-01-1-topic-organized-routing-full-catalog-rerank.json`](transcripts/raw/2026-06-01-1-topic-organized-routing-full-catalog-rerank.json)
