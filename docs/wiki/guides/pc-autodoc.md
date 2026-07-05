---
title: pc autodoc
slug: pc-autodoc
topic: autodoc
summary: `pc autodoc` is cut from the product
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:11099da8-f0fc-470d-9e28-d2aeba16b3e0
---

# pc autodoc

## Status

`pc autodoc` is cut from the product. The feature is judged harmful and poorly baked — it will hurt more than it helps — and is removed entirely. <!-- [^11099-8d0d5] -->

## Background

`pc autodoc` was a background command that auto-wrote wiki definition guides for code entities, triggered by dangling `[[slug]]` links in the wiki. Its intended role was to fill "Layer 2" — entity definitions that cannot be mined from transcripts because people never explicitly define the terms they use. The mechanism does not fire on the right entities, and output quality is below the threshold where it helps, so the recommendation is to cut the feature entirely. <!-- [^11099-650ce] -->
