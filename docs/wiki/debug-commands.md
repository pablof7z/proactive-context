---
title: Debug Commands
slug: debug-commands
topic: wiki-pipeline
summary: The CLI provides debug commands to investigate and iterate on extraction quality.  ### pc debug extract Process files for debugging extraction quality
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
---

# Debug Commands

## Debug Commands

The CLI provides debug commands to investigate and iterate on extraction quality.

### pc debug extract

Process files for debugging extraction quality. This command re-runs extraction on specified inputs so you can inspect the output and identify quality issues.

Usage: `pc debug extract <file> [--wiki-dir <dir>] [--no-wiki]`

This runs only the extraction step and displays the full prompt sent to the LLM, the raw LLM response, the parsed claims, and a summary — giving complete visibility into the extraction pipeline.

<!-- citations: [^5a147-17] -->
### pc debug transcript

Work with the same transcript the LLM uses during extraction. This lets you inspect the raw prompt context and LLM reasoning that feeds into extraction, making it easier to diagnose where extraction logic diverges from expectations.

Usage: `pc debug transcript <file>`

This prints the numbered transcript exactly as the LLM sees it, preserving the same formatting and content that feeds into the extraction step.

<!-- citations: [^5a147-18] -->
### Extraction investigation workflow
Investigation and iteration on extraction quality should be done in a background agent with results reported back. Rather than running debug steps synchronously in your local shell, the system dispatches a background agent to perform the extraction analysis and reports the findings back when complete. <!-- [^5a147-9] -->
