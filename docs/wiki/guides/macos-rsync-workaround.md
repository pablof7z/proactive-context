---
title: macOS rsync Workaround
slug: macos-rsync-workaround
topic: project-principles
summary: macOS ships `openrsync` (BSD), which does not support `--info=stats2` or `--files-from`
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-18
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
---

# macOS rsync Workaround

## macOS rsync Workaround

macOS ships `openrsync` (BSD), which does not support `--info=stats2` or `--files-from`. For large file-list transfers, use `tar -czf - -T list | ssh … tar -xzf -` instead. <!-- [^8eff6-82] -->
