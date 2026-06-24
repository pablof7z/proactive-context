---
type: episode-card
date: 2026-05-28
session: 5cf47d01-7a4e-4052-9948-8878a21b5b6a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5cf47d01-7a4e-4052-9948-8878a21b5b6a.jsonl
salience: root-cause
status: active
subjects:
  - vector-search
  - distance-metric
  - relevance-threshold
supersedes: []
related_claims: []
source_lines:
  - 658-1792
captured_at: 2026-06-17T12:12:17Z
---

# Episode: Vector search used L2 distance instead of cosine — garbage results for irrelevant queries

## Prior State

sqlite-vec's vec0 virtual table defaulted to L2 (Euclidean) distance, but all similarity thresholds and display logic assumed cosine distances. There was no relevance threshold — query always returned top_k results even when all were unrelated (e.g. 'nostr is a network' returned 8 chunks with distance ~1.23).

## Trigger

User tested query 'nostr is a network' and got irrelevant results. Investigation revealed orthogonal unit vectors returned distance 1.414 (√2) instead of the expected 1.0, confirming L2 metric. All prior similarity scores and thresholds were meaningless.

## Decision

Switched vec0 table creation to explicit distance=cosine. Added schema_version meta field (v2) so existing L2 databases are auto-recreated. Added max_distance threshold (default 0.75) filtering in vector_search so irrelevant results return 'No relevant chunks found.' Changed display from raw distance to similarity percentage (1−distance).

## Consequences

- All existing databases are dropped and rebuilt on next init (schema migration v1→v2)
- Cosine distance now behaves correctly: 0=identical, 1.0=orthogonal, >1.0=anti-correlated
- Irrelevant queries like 'nostr is a network' now return 'No relevant chunks found'
- Relevant queries show meaningful similarity percentages (e.g. 59% instead of raw distance 0.41)
- Threshold tuned to 0.75 cosine distance — borderline results (20-30% similarity) are filtered out

## Open Tail

- User's config reverted to embed_provider: openrouter which is unimplemented — needs to be reset to local
- Reranking may improve marginal query precision further

## Evidence

- transcript lines 658-1792

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-4-vector-search-used-l2-distance-instead.json`](transcripts/2026-05-28-4-vector-search-used-l2-distance-instead.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-4-vector-search-used-l2-distance-instead.json`](transcripts/raw/2026-05-28-4-vector-search-used-l2-distance-instead.json)
