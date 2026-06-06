---
title: Capture Agent
slug: capture-agent
topic: archeologist
summary: The capture agent does NOT use a full catalog broadcast
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:5a1472ae-2784-423d-8681-0bedcf6c165f
  - session:17c35740-f9e8-4b68-a281-400835f4c161
  - session:9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
---

# Capture Agent

## System Architecture

The capture agent does NOT use a full catalog broadcast. Full catalog broadcast was explicitly considered and rejected because it doesn't scale.

The capture agent operates as a tool-equipped loop. During each capture run, its agent loop calls wiki_list and wiki_read to inspect the wiki-so-far as needed. The agent receives tools dynamically rather than a pre-loaded list of wiki topics and entries. <!-- [^5a147-7] -->

Superseded agent claims are retained in the log for audit purposes but are not automatically rendered in the final output. <!-- [^5a147-8] -->

The `capture.start` event payload includes `date` and `session_id` fields. <!-- [^17c35-9] -->

Capture lifecycle events (`capture.start`, `capture.extract`, `capture.authority_tagging`, `capture.route`, `capture.agent_done`, `capture.done`) are rendered in the TUI feed with a descriptive line for each phase. Pipeline-internal events remain hidden from general views but still drive the 'current' stage label at the bottom. <!-- [^17c35-10] -->

RunCounters include a `started` counter that increments on `capture.start` events, with `too_short` redefined as `seen − started − triage_skip` and an `interrupted` bucket for sessions stopped mid-capture. <!-- [^5a147-3] [^5a147-6] -->

The quit path drains trailing events after `worker.join()` so that a q-interrupted session that still runs to completion writes its wiki reports as captured, not interrupted or too-short.

The TUI summary and line-log summary include an interrupted note (", N interrupted" when `interrupted() > 0`).

<!-- citations: [^5a147-7] [^5a147-8] [^17c35-9] [^17c35-10] [^5a147-3] [^5a147-6] [^9c66c-6] -->
