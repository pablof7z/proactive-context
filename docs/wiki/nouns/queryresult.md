---
type: noun-entry
slug: queryresult
name: "QueryResult"
origin: extracted
source_refs:
  - transcript:193-208
---

# QueryResult

Result of a query, optionally reranked. Fields: path (String), chunk_index (i64), content (String), content_hash (String), score (f64 in 0..1, computed as 1 - cosine_distance).
