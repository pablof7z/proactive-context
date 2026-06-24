---
title: Wiki Tool Contract
slug: wiki-tool-contract
topic: wiki-architecture
summary: The system operates on the principle that human time is irreplaceable and tokens are buyable
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a
  - session:105d3450-2ae4-4fc8-9c46-f74830a9dd97
  - session:7af90c87-0537-4784-b8ba-aaeae3786f59
  - session:26c909a1-6c07-4761-bac5-6e880cd7a063
---

# Wiki Tool Contract

## Guiding Philosophy

The system operates on the principle that human time is irreplaceable and tokens are buyable. It biases toward recall over precision, accepting some token cost and wiki churn rather than dropping nuance that would require a human to re-explain. <!-- [^aceca-7] -->

The wiki stores positive desired-state specifications (e.g., 'clicking an avatar opens a hovercard with user details'), not event logs or bug reports (e.g., 'avatar is broken'). <!-- [^aceca-8] -->

The project positions itself around 'your judgment is the asset; the machine is the cheap part' rather than anchoring on 'memory' or 'verbatim-cited injection' as the headline. Its differentiator is not any single feature but the theory of the job: capturing and compounding the one input a model can't generate — human judgment, taste, and discernment. Verbatim-cited injection earns its place as proof the system means it about treasuring human perspective, not as the headline anchor.

<!-- citations: [^aceca-7] [^aceca-8] [^105d3-8] -->
## Structural Anchors

The wiki tool contract uses section-heading addressing for stable structural anchors rather than a separate statement-ID registry or diffs. <!-- [^aceca-9] -->

## Enrich Operation

The enrich operation can restate a guide toward a cleaner, more complete positive spec, not just append paragraphs — the previous 'append-only, never full rewrite' constraint is removed. <!-- [^aceca-10] -->

## Tier Evolution

The implementation evolved away from `PRODUCT_MODEL.md` toward the wiki for per-project carry-forward, but the global tier was never built beyond the write-to-queue stub. <!-- [^7af90-5] -->

The wiki was migrated to ./docs/wiki/ and the code reads from that path; all consolidation work was done in the live location. <!-- [^26c90-14] -->
