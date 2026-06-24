---
type: episode-card
date: 2026-05-28
session: 9135070a-d269-45e6-8f71-27f2ef7246af
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9135070a-d269-45e6-8f71-27f2ef7246af.jsonl
salience: product
status: active
subjects:
  - openrouter-embeddings
  - embedding-provider
supersedes: []
related_claims: []
source_lines:
  - 417-678
captured_at: 2026-06-17T12:15:09Z
---

# Episode: OpenRouter embeddings implementation replaces stub

## Prior State

OpenRouterEmbedder was a stub that errored on use. Only local fastembed embeddings worked. The embed_provider config key existed but 'openrouter' was non-functional.

## Trigger

User explicitly directed: 'add embed on openrouter and test that it works'

## Decision

Implemented OpenRouterEmbedder with reqwest::blocking::Client calling https://openrouter.ai/api/v1/embeddings (OpenAI-compatible format). Added reqwest dependency to Cargo.toml. dimension() maps known model names (text-embedding-3-small → 1536, text-embedding-3-large → 3072) with a fallback warning.

## Consequences

- Both embedding providers (local and openrouter) now work end-to-end.
- Switching providers requires deleting the index DB because dimension mismatch is destructive (384-dim local vs 1536-dim OpenRouter).
- The ONNX reranker model (~100-200MB) still downloads on first generate use regardless of embedding provider.

## Open Tail

- Dimension migration on provider switch is still manual and destructive — spec mentions a future 'incremental dimension migration or warning.'
- A --no-rerank flag for generate was proposed but not yet implemented.

## Evidence

- transcript lines 417-678

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-2-openrouter-embeddings-implementation-replaces-stub.json`](transcripts/2026-05-28-2-openrouter-embeddings-implementation-replaces-stub.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-2-openrouter-embeddings-implementation-replaces-stub.json`](transcripts/raw/2026-05-28-2-openrouter-embeddings-implementation-replaces-stub.json)
