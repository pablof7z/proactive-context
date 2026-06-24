---
title: Global Lessons Store
slug: global-lessons-store
topic: data-persistence
summary: The global index at `~/.proactive-context/global/index.db` is never populated; capture writes to the markdown queue rather than the index
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:7af90c87-0537-4784-b8ba-aaeae3786f59
---

# Global Lessons Store

## Global Lessons Store

The global index at `~/.proactive-context/global/index.db` is never populated; capture writes to the markdown queue rather than the index. The `inject` command never reads from the global store; the only `global` reference in `inject.rs` is an unrelated gitignore flag. <!-- [^7af90-2] -->

To make proactive-context genuinely carry global lessons forward, two missing pieces must be built: (a) a promotion path from `pending-lessons.md` into `global/index.db` or a global wiki, and (b) wiring `inject` to query the global store alongside the project wiki. <!-- [^7af90-3] -->
