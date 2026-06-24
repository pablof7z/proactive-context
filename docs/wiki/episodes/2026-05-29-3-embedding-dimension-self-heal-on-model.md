---
type: episode-card
date: 2026-05-29
session: 658f4c79-7e15-49f1-a803-41a4d58866eb
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/658f4c79-7e15-49f1-a803-41a4d58866eb.jsonl
salience: root-cause
status: active
subjects:
  - db-schema
  - embed-dimension
  - auto-reindex
supersedes: []
related_claims: []
source_lines:
  - 3222-3892
captured_at: 2026-06-17T12:32:04Z
---

# Episode: Embedding dimension self-heal on model switch

## Prior State

Switching embedding models (e.g. OpenRouter 1536-dim → local 384-dim) caused a hard crash: 'Dimension mismatch for query vector... Expected 1536 dimensions but received 384.' The meta.embed_dim value could drift out of sync with the actual vec_chunks FLOAT[N] declaration, making manual DB deletion the only recovery path. Initial fix (checking only meta.embed_dim) was insufficient because meta could already say 384 while the table was still FLOAT[1536].

## Trigger

User hit dimension mismatch after switching embed_provider to local (line 3222). Initial fix failed because it trusted meta over the actual table schema (lines 3769-3779: meta=384 but vec_chunks=FLOAT[1536]). User insisted the system should self-heal without manual intervention (line 3280).

## Decision

init_schema now reads FLOAT[N] directly from sqlite_master to detect the actual vec_chunks dimension. On mismatch with the current embedder's dimension, it drops the virtual table and recreates it at the correct dimension, then logs 'embed dim changed (X → Y), wiping index for re-embedding'. Also removed a spurious DELETE FROM chunks (no such table — data lives in vec_chunks).

## Consequences

- Future embed model switches self-heal on next DB open — no manual DB deletion needed
- Daemon re-indexes automatically after dimension change; users see a log message and must wait for re-indexing but get no crashes
- Source of truth for dimension is now the actual sqlite_master DDL, not the meta table

## Open Tail

*(none)*

## Evidence

- transcript lines 3222-3892

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-3-embedding-dimension-self-heal-on-model.json`](transcripts/2026-05-29-3-embedding-dimension-self-heal-on-model.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-3-embedding-dimension-self-heal-on-model.json`](transcripts/raw/2026-05-29-3-embedding-dimension-self-heal-on-model.json)
