---
title: Event Log
slug: event-log
topic: event-log
summary: The event log is a single global append-only file at `~/.proactive-context/logs/events.jsonl` with atomic `O_APPEND` sub-`PIPE_BUF` writes and no hot-path locki
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
---

# Event Log

## Overview

The event log is a single global append-only file at `~/.proactive-context/logs/events.jsonl` with atomic `O_APPEND` sub-`PIPE_BUF` writes and no hot-path locking. The event taxonomy maps one request's lifecycle: `inject.start → query.start → retrieve.* → generate.* → inject.done`. Latency is the headline metric on `inject.done` with per-stage breakdown at `-v` and amber/red thresholds for slow injects. Capture events are visually marked as the slow lane with `S`-prefixed IDs and diamond glyphs so their 15s timings don't read as errors. <!-- [^1fe0f-0513d] -->

## Emission Points

`query.start`/`retrieve.hit` emit from `run_query` itself so standalone CLI `query`/`generate` also emit for free, which is acceptable and desirable for a live view. <!-- [^1fe0f-e3458] -->

## Wiki Event Types

New wiki event types are: `wiki.index_read`, `generate.tool_call`, `guide.read`, `link.follow`, `guide.create`, `guide.update`, `select.shortcircuit`. <!-- [^1fe0f-3db29] -->
