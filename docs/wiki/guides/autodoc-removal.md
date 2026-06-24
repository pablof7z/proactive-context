---
title: Autodoc Removal
slug: autodoc-removal
topic: wiki-architecture
summary: The `pc autodoc` command and its related dead code should be removed from the project
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-15
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:11099da8-f0fc-470d-9e28-d2aeba16b3e0
  - session:64c94ab4-45c5-4746-9d50-678dcfa6851c
---

# Autodoc Removal

## Removal of `pc autodoc`

The `pc autodoc` command and its related dead code should be removed from the project. The command's trigger mechanism relies on dangling `[[slug]]` links that fire rarely and only for edge cases, rather than filling the intended 'Layer 2 gap.' Additionally, the `pc autodoc` grep command is hardcoded to `--include=*.rs`, producing garbage results for any non-Rust project. The old top-level commands (`pc capture`, `pc inject`, `pc session-start`, `pc statusline`) are removed entirely with no backward compatibility preserved.

<!-- citations: [^11099-1] [^64c94-2] -->
