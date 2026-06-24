---
title: CLI Daemon Lifecycle
slug: cli-daemon-lifecycle
topic: cli-daemon
summary: The init command forks into the background; if the daemon is already running, it exits 0 silently
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-06-16
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:5cf47d01-7a4e-4052-9948-8878a21b5b6a
  - session:9135070a-d269-45e6-8f71-27f2ef7246af
  - session:81d75227-ddb9-446a-9faa-00434dce6d2e
---

# CLI Daemon Lifecycle

## Init Command

The init command forks into the background; if the daemon is already running, it exits 0 silently. When no project index exists and more than 5 indexable files are found, the daemon starts silently in the background regardless of LOC, with no file listing, prompts, or noise.

<!-- citations: [^5cf47-1] [^81d75-1] -->
## Stop Command

The stop command sends SIGTERM, waits up to 2 seconds for graceful exit, then sends SIGKILL, and cleans up the PID file either way. <!-- [^5cf47-2] -->

## Ps Command

The ps command scans ~/.proactive-context/projects/*/daemon.pid and lists PID, uptime, and directory for every live daemon, automatically cleaning stale PID files of dead processes. <!-- [^5cf47-3] -->

## Stats Command

The CLI exposes a `stats` command that opens the database and prints file and chunk counts. Currently it only prints file and chunk counts, whereas the spec calls for showing embedding model, daemon status, DB size, and activity. A `--watch` flag for live-updating stats is specified but not yet implemented. The spec lists `stats` and `status` interchangeably, but no `status` alias exists. <!-- [^91350-1] -->
