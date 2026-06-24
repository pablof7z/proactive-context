---
title: Stats Command
slug: stats-command
topic: cli-daemon
summary: The `stats` command displays colorized output with daemon status (green â / red â), file/chunk counts, DB size, and embedding model+dim.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:0bf0fe1c-fbf5-497e-b286-e364266abf05
---

# Stats Command

## Output

The `stats` command displays colorized output with daemon status (green ● / red ●), file/chunk counts, DB size, and embedding model+dim. <!-- [^0bf0f-6] -->

The `stats --watch` flag refreshes stats every second in-place using cursor repositioning with no flicker. <!-- [^0bf0f-7] -->

## Implementation

The implementation adds an IndexStats struct and index_stats_full() in db.rs, a daemon_pid() helper in daemon.rs, and a print_stats function with fmt_bytes helper in main.rs. <!-- [^0bf0f-8] -->
