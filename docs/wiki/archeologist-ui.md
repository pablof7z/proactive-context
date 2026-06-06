---
title: Archeologist UI
slug: archeologist-ui
topic: archeologist
summary: "The `FeedLine` struct includes a `detail: String` field for displaying full content in the detail overlay.  The `RunView` struct tracks `detail_open: Option<Str"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:17c35740-f9e8-4b68-a281-400835f4c161
  - session:9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
---

# Archeologist UI

## Data Model

The `FeedLine` struct includes `is_conversation: bool` and `session_id: Option<String>` fields, set appropriately in `feed_line_for_event` for "Reading conversation" lines, plus a `detail: String` field for displaying content in the detail overlay.

The `RunView` struct tracks `detail_open: Option<String>`, `detail_scroll: usize` for overlay scrolling, and `transcript_by_session: HashMap<String, String>` for per-session transcript storage. (Previously: tracked `last_sidecar: Option<String>` for the drill-down feature.)

<!-- citations: [^17c35-6] [^9c66c-1] -->
## Interaction

Pressing Enter on a selected "Reading conversation" feed line opens a scrollable detail overlay showing the EXTRACT transcript (the full conversation sent to the LLM, rendered role-by-role). The overlay supports vertical scrolling with Up/Down keys when open, and long transcript content does not clip silently. Press Esc to close the overlay. (Previously: Pressing Enter on a selected feed line opened a full-screen overlay showing the complete detail text.)

The feed window uses bottom-anchored windowing where the cursor moves within the window and only scrolls the window once the cursor climbs above the top, with unified cursor-index math in a single helper used by render, Enter, and scroll. <!-- [^9c66c-3] -->

When the detail overlay is open, the help line mentions scrolling capability. <!-- [^9c66c-5] -->

<!-- citations: [^17c35-7] [^9c66c-2] -->
## Sidecar File

The `llm.response` event's `sidecar` field contains the path to a JSON file with the full prompt messages and completion content. The transcript content is captured eagerly from the `llm.response` event payload (not stored as a file path for lazy reading) because sidecar files are overwritten by subsequent LLM calls within the same session. The event loop populates `transcript_by_session` with insert-if-absent semantics on every `llm.response` sidecar, right after `counters.apply`. (Previously: the `last_sidecar` path was tracked for future drill-down use.)

<!-- citations: [^17c35-8] [^9c66c-4] -->
