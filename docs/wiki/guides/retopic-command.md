---
title: Retopic Command
slug: retopic-command
topic: wiki-architecture
summary: The `--retopic` command performs a single blocking, non-streaming HTTP call to the cloud LLM with the entire guide catalog, with up to a 600-second timeout and
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

# Retopic Command

## Retopic Command

The `--retopic` command performs a single blocking, non-streaming HTTP call to the cloud LLM with the entire guide catalog, with up to a 600-second timeout and 3 retries. The call sends a small prompt (~1,700 tokens total, ~5,600 chars for the catalog) consisting of one short line per guide (index, slug, title, summary, current topic) without guide bodies. The latency of the `--retopic` call is dominated by the model's hidden chain-of-thought reasoning and cloud queueing, not the input or output payload size. <!-- [^6faa5-1] -->

A `with_heartbeat()` helper spawns a side thread that prints a braille spinner and elapsed seconds to stderr while the retopic blocking LLM call runs, then clears its line to avoid colliding with stdout. <!-- [^6faa5-2] -->

Retopic tends to collapse ~27 guides into approximately 4 broad topics, which is a known separate issue from the hang latency. <!-- [^6faa5-3] -->

A lighter, non-reasoning model (e.g., `--model ollama:gpt-oss:cloud`) can be used for the taxonomy pass since it is a bucketing task that does not require heavy reasoning. <!-- [^6faa5-4] -->
