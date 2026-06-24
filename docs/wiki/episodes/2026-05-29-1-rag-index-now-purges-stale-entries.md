---
type: episode-card
date: 2026-05-29
session: 4b94dc35-2335-439a-8d0f-79ab19f5efe1
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/4b94dc35-2335-439a-8d0f-79ab19f5efe1.jsonl
salience: product
status: active
subjects:
  - rag-index-stale-purge
  - full-index-sweep
  - indexed-paths-db
supersedes: []
related_claims: []
source_lines:
  - 1-62
  - 143-200
captured_at: 2026-06-17T12:54:00Z
---

# Episode: RAG index now purges stale entries for files deleted while daemon was offline

## Prior State

The file watcher correctly deleted chunks for files removed while the daemon was running, but full_index only walked the filesystem and skipped re-embedding existing files — it never swept the DB for entries whose source files had vanished. Files deleted while the daemon was offline left orphaned chunks in the RAG index indefinitely.

## Trigger

User asked whether deleted markdown files are removed from the RAG; investigation revealed the gap that offline deletions were not handled.

## Decision

Added an indexed_paths() DB function and a stale-entry sweep at the top of full_index: before walking the filesystem, the function now queries all indexed paths from the DB and deletes any whose files no longer exist on disk.

## Consequences

- RAG index is now consistent with the filesystem even after daemon downtime
- A new DB function indexed_paths() is part of the public db API
- full_index incurs an extra DB query + path-existence checks on every run, but this is cheap compared to embedding cost

## Open Tail

*(none)*

## Evidence

- transcript lines 1-62
- transcript lines 143-200

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-rag-index-now-purges-stale-entries.json`](transcripts/2026-05-29-1-rag-index-now-purges-stale-entries.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-rag-index-now-purges-stale-entries.json`](transcripts/raw/2026-05-29-1-rag-index-now-purges-stale-entries.json)
