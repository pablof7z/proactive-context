---
title: LLM Observability
slug: llm-observability
topic: llm-observability
summary: The event JSONL log records all steps throughout LLM generation, including cost metadata from OpenRouter (usage.cost is always present in the response body with
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-16
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:9af6a8f7-5ec5-420f-9110-fdf509d30c2b
  - session:54cada63-dcb1-4088-9838-22639779ca06
---

# LLM Observability

## Event Logging

The event JSONL log records all steps throughout LLM generation, including cost metadata from OpenRouter (usage.cost is always present in the response body with no opt-in needed). Both OpenRouter and Ollama LLM call paths in inject.rs are instrumented to log llm.request/llm.response events and write sidecar files. The separate /generation endpoint does not work reliably; cost metadata comes only from the main response body. RunCounters accumulates usage from llm.response events using flat prompt_tokens, completion_tokens, and cost_usd fields, rather than only the nested usage.* shape.

<!-- citations: [^9af6a-5] [^54cad-2] -->
## Sidecar Files

A sidecar JSON file is written at ~/.proactive-context/logs/llm_turns/<req>-t<turn>.json containing the full prompt messages array, response text, and usage/cost data for each LLM turn. Turn numbering uses t1 for the select call and t2 for the compile call to avoid overwriting the same t0 file. <!-- [^9af6a-6] -->

## TUI Display

The tail command's TUI shows what was sent to the model and what it generated at each step. The llm.request event glyph is ⇢ and llm.response glyph is ⇠, showing model, turn number, tokens, cost, and response preview. The TUI modal renders the full messages[] array with role labels ([system], [user], [assistant]) and the full response, with no hard line caps — the user scrolls through the entire content. The TUI modal detail pane supports scrolling with ↑/↓ or j/k keys, Space/PgDn for 20-line jumps, and ←/→ or h/l for navigating between sibling events in the request trace. <!-- [^9af6a-7] -->
