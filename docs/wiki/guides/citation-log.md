---
title: Citation Log
slug: citation-log
topic: citation-system
summary: The citation log is append-only and must stay out of the retrieval/embedding index and out of inject
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
  - session:d00d68d4-f98d-46b7-be4d-51610d05bf3b
---

# Citation Log

## Citation Log

The citation log is append-only and must stay out of the retrieval/embedding index and out of inject. It is write-mostly, read-only-for-audit. The writer calls `create_dir_all` before writing to handle the case where the wiki directory does not yet exist on the first session capture.

<!-- citations: [^aceca-6] [^d00d6-2] -->
