---
title: Retopic
slug: retopic
topic: wiki-pipeline
summary: The `--retopic` command reassigns topics across the entire guide catalog by making a single blocking, non-streaming HTTP call to a cloud LLM
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:6faa5ac2-c7f5-4c16-97bf-942f2c9b1098
---

# Retopic

## Overview

The `--retopic` command reassigns topics across the entire guide catalog by making a single blocking, non-streaming HTTP call to a cloud LLM. There is no intermediate progress output during the call itself. <!-- [^6faa5-1] -->


The retopic output is a JSON taxonomy of approximately 600 tokens containing per-guide topic assignments. <!-- [^6faa5-6] -->
## Request Configuration

The LLM call uses `stream: false` with a 600-second timeout and up to 3 retries. A lighter non-reasoning model can be specified via the `--model` flag to reduce taxonomy pass latency. Note: retopic has a known issue of collapsing guides into approximately 4 broad topics that may be too coarse.

<!-- citations: [^6faa5-2] [^6faa5-7] -->
## User Feedback During Execution

Since retopic is a doctor batch job and not part of the capture pipeline, `pc tail` does not show its progress. To provide user feedback during the blocking LLM call, a `with_heartbeat()` helper prints a braille spinner plus elapsed seconds to stderr, then clears its line to avoid colliding with stdout output. <!-- [^6faa5-3] -->

## Prompt Structure

The retopic call sends approximately 1,700 tokens total in a single prompt. The prompt includes system instructions and a catalog of all guides, with one short line per guide containing the index, slug, title, summary, and current topic only. Full guide bodies are not included. <!-- [^6faa5-4] -->

## Performance Characteristics

Call latency is dominated by model-side reasoning and cloud queueing/cold-start, not by payload size. <!-- [^6faa5-5] -->
