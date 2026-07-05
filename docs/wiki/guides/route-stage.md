---
title: ROUTE Stage
slug: route-stage
topic: capture-pipeline
summary: ROUTE enforces one coherent topic per guide â the ROUTE altitude fix â defining a guide as one coherent topic a reader opens under one title (a subsystem/co
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-06
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
  - session:be9ee788-301d-4758-9260-69dce3ae35b9
  - session:0ce97719-96b9-4ab3-90b8-d9f66e493bff
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
---

# ROUTE Stage

## Definition

ROUTE enforces one coherent topic per guide — the ROUTE altitude fix — defining a guide as one coherent topic a reader opens under one title (a subsystem/concern-level chapter that accumulates many claims). New guides are created with a real title and summary in their frontmatter (derived from the first statement), so subsequent ROUTE calls can see what existing guides cover. The ROUTE prompt must not state a target guide count for a healthy project wiki. The project maintains a spec doc for topic-organized routing and staleness retirement.

<!-- citations: [^26c90-30ecd] [^be9ee-d312d] [^0ce97-4d1d5] -->
## Mechanism

The ROUTE stage uses a retrieve-then-rerank pipeline: vector embedding of claims, embedding cosine recall over live guides (RECALL) followed by a constrained LLM choice among top-K candidates (RERANK). The embedding router performs in-memory cosine similarity over live guides to produce top-K candidates, then a constrained LLM picks the home slug or NEW. Embedding recall uses the live on-disk guide frontmatter at route time, never the checkpoint-stale `index.db`, so a session sees siblings written earlier in the same window.

After routing assigns each claim to a target guide, RECONCILE groups claims by target guide and processes one guide at a time sequentially, showing the model the current guide body alongside the routed claims so contradictions are caught and resolved rather than accreted. <!-- [^5a147-2c382] -->

<!-- citations: [^be9ee-64087] [^0ce97-040f7] -->
## Tuning Knobs

The embedding router's tuning knobs are `PC_ROUTE_TOP_K` (default 8) and `PC_ROUTE_TAU` (default 0.30). <!-- [^be9ee-a65b3] -->

## Implementation

`route_recall.rs` is the module implementing cosine similarity, guide representation (title+summary), and candidate recall for the embedding router. <!-- [^be9ee-0dfe3] -->

## Routing as Primary Bottleneck

Routing is the primary bottleneck in capture quality, confirmed empirically three times across multiple experiments. Capture-time topic routing produces a 1:1 guide-to-topic ratio (every guide invents its own topic), failing to achieve grouping. <!-- [^be9ee-eae45] -->
