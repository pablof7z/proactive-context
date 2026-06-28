---
type: episode-card
date: 2026-06-26
session: 151a7e32-2bff-4b31-9196-dd6060fdb411
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/151a7e32-2bff-4b31-9196-dd6060fdb411.jsonl
salience: reversal
status: active
subjects:
  - embedding-provider
  - openrouter
  - local-to-remote
  - vector-dimension
supersedes: []
related_claims: []
source_lines:
  - 1399-1598
captured_at: 2026-06-26T07:32:22Z
---

# Episode: Switch to OpenRouter embeddings (text-embedding-3-small, 1536-dim)

## Prior State

Embeddings via local fastembed (all-MiniLM-L6-v2, 384-dim, CPU-intensive; 469% CPU during cold indexing).

## Trigger

User request: 'configure my pc to use openroute's embedder'. Verified OpenRouter /api/v1/embeddings endpoint actually exists (live test: HTTP 200 with real vectors returned).

## Decision

Change embed_provider to 'openrouter' and embed_model to 'openai/text-embedding-3-small' (1536-dim). Existing project indexes auto-wipe and rebuild at new dimension on next daemon run via existing open_db dimension-mismatch detection.

## Consequences

- Embedding becomes remote network I/O (~100–300ms per call) instead of local CPU (eliminates 469% fastembed CPU load).
- Existing project indexes auto-rebuild at 1536-dim on next index event; one-time cost per project (~$0.02 for this repo's 4500 chunks).
- Content-hash skip (Arc 2) now prevents redundant API calls in addition to CPU saves.
- Hot-path latency remains acceptable: embed calls fit within existing 25s inject budget.
- Configuration persisted to ~/.proactive-context/config.json; backup saved.

## Open Tail

- Minor cosmetic: meta.embed_provider field shows 'local' even though stored vectors are 1536-dim (dimension, not label, drives wipe/rebuild logic; harmless but stats label lags reality).

## Evidence

- transcript lines 1399-1598

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-3-switch-to-openrouter-embeddings-text-embedding.json`](transcripts/2026-06-26-3-switch-to-openrouter-embeddings-text-embedding.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-3-switch-to-openrouter-embeddings-text-embedding.json`](transcripts/raw/2026-06-26-3-switch-to-openrouter-embeddings-text-embedding.json)
