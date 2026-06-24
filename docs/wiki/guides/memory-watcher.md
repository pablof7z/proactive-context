---
title: Memory Watcher
slug: memory-watcher
topic: cli-daemon
summary: When the user says 'sample <number>', it is interpreted as the macOS profiler `sample <pid>` and executed immediately without asking clarifying questions.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:ad1a2cf7-6183-46ba-a68c-4c770ebc1261
---

# Memory Watcher

## Behavior

When the user says 'sample <number>', it is interpreted as the macOS profiler `sample <pid>` and executed immediately without asking clarifying questions. <!-- [^ad1a2-1] -->

The memory watcher monitors `pc` processes with RSS exceeding 500 MB. <!-- [^ad1a2-2] -->

The watcher polls for matching processes every 15 seconds via `ps`. <!-- [^ad1a2-3] -->

Upon detecting a `pc` process over 500 MB, the watcher captures `ps` details and a 5-second `sample` profile to `/tmp/pc-mem-watch/hit-<timestamp>-pid<PID>.txt`. <!-- [^ad1a2-4] -->

The watcher is one-shot by default, exiting after the first hit to wake the assistant for review, rather than looping for repeated hits. <!-- [^ad1a2-5] -->
