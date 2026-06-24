---
title: Tail TUI
slug: tail-tui
topic: cli-daemon
summary: Error messages in the tail TUI display up to 250 characters before being truncated.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-06
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:a94806b5-fc73-42cf-8bd2-e93aad8dabd2
  - session:31cc17e6-16bf-4c63-a6b4-1b1d67795aa1
  - session:9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
---

# Tail TUI

## Error Messages

Error messages in the tail TUI display up to 250 characters before being truncated.

<!-- citations: [^a9480-1] [^31cc1-1] -->

## Context Selector

The catalog provided to the context selector must display with line separators rendered as visual line breaks. <!-- [^31cc1-2] -->

## Modals and Sidecars

The payload display modal (render_modal_event_detail) splits on \n escape sequences to properly render embedded newlines, with a 25-line display limit (up from 8) to accommodate expanded content. The LLM sidecar message rendering (llm_sidecar_lines) respects actual newlines in message content before character chunking, so multi-line content displays with proper structure. The inject sidecar rendering (inject_sidecar_lines) is a separate function with the same newline-respecting logic as the LLM sidecar, used for displaying select/compile turn sidecars. <!-- [^31cc1-3] -->

## Feed Rendering and Navigation

The archeologist TUI feed renders capture lifecycle events (capture.start, capture.extract, capture.authority_tagging, capture.route, capture.agent_done, capture.done) as narrated lines instead of remaining silent between session start and the first wiki mutation. The feed window is bottom-anchored: the cursor moves within the visible window and only scrolls the window when the cursor reaches the top edge, unified via a feed_window() helper used by render, Enter, and scroll-clamping. When scrolled up during a live run, the feed_scroll is incremented in lock-step with incoming events (feed_scroll += 1 when not at bottom) so the cursor stays on the same logical line rather than drifting toward newer items. The feed title shows 'feed · line N/M' whenever the user is scrolled up, indicating the cursor's position in the total feed. Pressing Enter on a 'Reading conversation' feed line opens a scrollable detail overlay showing the full EXTRACT transcript (the line-numbered conversation sent to the LLM), rendered role-by-role, with ↑/↓ scrolling. <!-- [^9c66c-4] -->

## Current Region Phase Label

The archeologist TUI current-region displays a live phase label (starting → extracting claims → tagging authority → routing to guides → reconciling guides → writing wiki → rebuilding index) driven by the session's own events, with a '· waiting on model' marker between an llm.request and its response. <!-- [^9c66c-5] -->
