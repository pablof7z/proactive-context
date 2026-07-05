---
type: episode-card
date: 2026-06-01
session: 8fa18555-86b6-492d-9b13-1865774df99c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/8fa18555-86b6-492d-9b13-1865774df99c.jsonl
salience: architecture
status: active
subjects:
  - topic-organized-routing
  - route-rerank
  - guide-frontmatter
  - wiki-index
supersedes: []
related_claims: []
source_lines:
  - 1211-1250
  - 1633-1640
  - 2010-2038
captured_at: 2026-06-29T11:59:23Z
---

# Episode: Phase 2 topic-organized routing: RERANK gets full-catalog view, guides gain topic field

## Prior State

ROUTE/RERANK only saw ~8 cosine-recalled candidates per claim with no global catalog view, causing it to mint parallel near-duplicate guides for topics it couldn't see (172 flat guides for nostr). Guides had no topic field; _index.md was a flat ungrouped table. Phase 1 had shipped only a prompt altitude fix (one coherent topic per guide, no count target), but Phase 2 (topic metadata + full-catalog RERANK) was designed but unbuilt.

## Trigger

User ran archeologist against podcast project, noticed output was all flat guides in docs/wiki with no topic organization ('what happened to the topics?'). Assistant explained only Phase 1 had shipped and Phase 2 was unbuilt. User directed: 'do it'.

## Decision

Implemented Phase 2 topic-organized routing: (1) added `topic: String` to GuideFrontmatter and IndexRow, parsed from frontmatter and serialized only when non-empty for back-compat; (2) RERANK now receives a full-catalog grouped-by-topic view alongside per-claim candidates so it can route to existing topic areas instead of minting blind; (3) RouteDecision gains a topic field — RERANK emits topic per route decision; (4) existing guides get topic back-filled on their next RECONCILE update; (5) _index.md rewritten to topic-grouped sections with a machine-parseable 7th-column topic field.

## Consequences

- Next capture run against any project produces topic-tagged guides with a grouped _index.md
- RERANK can now see the entire existing catalog by topic, reducing parallel near-duplicate slug creation
- Back-compat preserved: topic field serializes only when non-empty, so existing wikis without topic still parse and round-trip
- 73/73 unit tests pass; topic field threaded through all 5 new_guide call sites, apply_reconcile_op signature, doctor.rs and route_recall.rs test helpers
- Physical topics/ directory layout was rejected in favor of metadata-only topic (avoids touching 10+ call sites and podcast docs/wiki/topics/ collision)

## Open Tail

- Staleness retirement (Phase 3, demote-not-delete under keep-everything) remains designed but unbuilt
- Validation needed: confirm topic routing reduces guide count on a fresh rebuild vs Phase 1 altitude-only baseline
- wiki-doctor still needed to merge existing fragmentation like clip-boundary-resolver vs clip-boundary-resolution

## Evidence

- transcript lines 1211-1250
- transcript lines 1633-1640
- transcript lines 2010-2038

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-01-1-phase-2-topic-organized-routing-rerank.json`](transcripts/2026-06-01-1-phase-2-topic-organized-routing-rerank.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-01-1-phase-2-topic-organized-routing-rerank.json`](transcripts/raw/2026-06-01-1-phase-2-topic-organized-routing-rerank.json)
