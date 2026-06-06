---
title: Wiki Pipeline
slug: wiki-pipeline
topic: archeologist
summary: "The wiki-pipeline implements a 4-stage processing flow: EXTRACT → ROUTE → RECONCILE → HISTORY."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:17c35740-f9e8-4b68-a281-400835f4c161
  - session:9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
---

# Wiki Pipeline

## Pipeline Overview

The wiki-pipeline implements a 4-stage processing flow: EXTRACT → ROUTE → RECONCILE → HISTORY.

The EXTRACT stage reads raw transcripts and produces atomic cited claims with author tagging and evidence line ranges. It does not access existing wiki content. The EXTRACT prompt includes a sweep-completeness nudge to capture late-session reversals.

The ROUTE stage uses embedding RAG to find top-K candidate guides by semantic similarity, then applies reasoning rerank to catch cases where embedding similarity fails. ROUTE outputs per claim are a target slug or new topic designation.

The RECONCILE stage groups claims by target guide and processes one guide at a time sequentially. Its per-claim decisions are add, revise, remove, or propose-new. Sequential reconciliation is required because parallel workers would each see a stale snapshot and independently append. When applying operations, the RECONCILE stage emits structured log events: `wiki.create`, `wiki.add_statement`, or `wiki.revise_statement` after each operation completes. A live processing indicator in the "current" region shows the active stage of the active session (starting → extracting claims → tagging authority → routing to guides → reconciling guides → writing wiki → rebuilding index) with a "· waiting on model" marker between llm.request and llm.response.

The HISTORY stage records supersession chains with old claims retained but marked superseded, never deleted. Superseded user claims are rendered as terse breadcrumbs for genuine mind-changes.

The wiki index is not provided to EXTRACT in live capture due to causing extraction failures from prompt truncation.

<!-- citations: [^5a147-4] [^5a147-13] [^5a147-14] [^5a147-15] [^5a147-5] [^5a147-11] [^5a147-12] [^17c35-12] [^5a147-19] [^9c66c-8] -->
