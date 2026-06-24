---
title: Embedding
slug: embedding
topic: vector-search
summary: The embedding provider is set to 'local' using the 'all-MiniLM-L6-v2' model (384 dimensions), replacing the previous OpenRouter embedder (1536 dimensions)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-14
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:658f4c79-7e15-49f1-a803-41a4d58866eb
  - session:5556972e-d2e7-42cf-9567-68edde7382b2
  - session:f62ced47-ebf8-4f18-861f-4a9fd087b787
  - session:f4556ab3-b961-4730-872d-697277e59a34
---

# Embedding

## Embedding Provider

The embedding provider is set to 'local' using the 'all-MiniLM-L6-v2' model (384 dimensions), replacing the previous OpenRouter embedder (1536 dimensions). The embedder processes chunks in batches of 32 per ONNX inference pass to keep memory flat regardless of input size. Both the embedding model and the reranker model use the configured cache directory. The embedder must be built once and shared between the claims-log tap and the ROUTE recall phase rather than built independently in each phase. Building the ONNX embedder model twice causes approximately 1.6 GB peak RSS because each 86 MB model load inflates to 500–800 MB in working memory via ONNX Runtime (thread pools, inference graph, activation buffers). Sharing a single embedder instance cuts peak RSS for the embedder roughly in half compared to building it twice.

<!-- citations: [^658f4-1] [^55569-1] [^f62ce-2] [^f4556-1] -->
