---
title: Vector Search
slug: vector-search
topic: vector-search
summary: The chunker walks back to the nearest valid character boundary before slicing, preventing panics on multi-byte UTF-8 characters
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:5cf47d01-7a4e-4052-9948-8878a21b5b6a
  - session:9135070a-d269-45e6-8f71-27f2ef7246af
  - session:ed37c932-17ed-4003-935e-d232e9195c59
  - session:658f4c79-7e15-49f1-a803-41a4d58866eb
  - session:4b94dc35-2335-439a-8d0f-79ab19f5efe1
---

# Vector Search

## Chunking

The chunker walks back to the nearest valid character boundary before slicing, preventing panics on multi-byte UTF-8 characters. <!-- [^5cf47-6] -->

## Distance Metric

Vector search uses explicit cosine distance in the sqlite-vec schema (0 = identical, 1.0 = orthogonal, >1.0 = unrelated). <!-- [^5cf47-7] -->

## Schema Migration

A schema_version in the meta table triggers automatic drop and recreation of databases created with the old L2 distance metric. When the embedding dimension changes, the system detects the mismatch by reading the actual FLOAT[N] dimension from sqlite_master (not from meta), wipes the stale vec_chunks table, and re-embeds automatically rather than requiring manual intervention.

<!-- citations: [^5cf47-8] [^91350-7] [^658f4-9] -->
## Relevance Filtering

Vector search filters out results with distance >= 0.75 as not relevant, via a max_distance parameter on vector_search. <!-- [^5cf47-9] -->

## Similarity Display

Query results display similarity as a percentage calculated as (1 - distance). <!-- [^5cf47-10] -->

## Stats Command

The Stats command handler calls db::ensure_vec_extension() before opening the database connection to load the sqlite-vec extension. <!-- [^91350-3] -->

## Embedding Providers

OpenRouter embedding is supported, using reqwest::blocking::Client to call the OpenAI-compatible https://openrouter.ai/api/v1/embeddings endpoint. The OpenRouter embedder's dimension() method maps known model names (e.g., text-embedding-3-small → 1536, text-embedding-3-large → 3072) with a fallback warning for unknown models. The build_embedder() function reloads the ONNX model from disk on every query or generate invocation. <!-- [^91350-4] -->

## Content Hashing

The daemon stores per-chunk SHA-256 hashes but does not use them for content-hash skipping; it deletes and re-embeds an entire file on every change. <!-- [^91350-5] -->

## Reranker Scores

Reranker relevance scores are discarded; the original vector distance is shown instead. <!-- [^91350-6] -->

## Query Options

The query command supports --rerank for cross-encoder reranking and --global to search the global lessons index at ~/.proactive-context/global/index.db. <!-- [^ed37c-10] -->

## Stale Entry Handling

When a markdown file is deleted while the watcher daemon is running, the corresponding chunks are removed from the RAG index. The full_index function performs a stale-entry sweep at the start of the run, purging any DB entry whose file no longer exists on disk, ensuring deletions that occurred while the daemon was offline are resolved. The indexed_paths function in db.rs lists all indexed paths for use in this sweep. <!-- [^4b94d-1] -->
