---
type: noun-entry
slug: embed-provider-embedder
name: "embed_provider / embedder"
origin: extracted
source_refs:
  - transcript:1405-1441
  - transcript:1579-1592
---

# embed_provider / embedder

The backend that computes vector embeddings for markdown chunks; can be 'local' (fastembed, offline, e.g., all-MiniLM-L6-v2 @ 384-dim) or 'openrouter' (remote API, e.g., openai/text-embedding-3-small @ 1536-dim); dimension mismatch triggers auto-wipe + re-index
