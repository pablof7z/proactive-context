---
title: Oracle Command
slug: oracle-command
topic: wiki-architecture
summary: The `pc oracle` subcommand provides full-wiki large-context operations
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-01
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:8fa18555-86b6-492d-9b13-1865774df99c
---

# Oracle Command

## pc oracle

The `pc oracle` subcommand provides full-wiki large-context operations. GitHub issue #1 tracks this feature exploration. <!-- [^8fa18-1] -->

The `pc oracle <question>` subcommand concatenates all wiki guides into a single context dump and sends them to a user-configured big-context model for full-wiki Q&A. <!-- [^8fa18-2] -->

The `pc oracle --recompile` mode sends the entire wiki to a model in one pass to identify contradictions, merge near-duplicates, fill gaps, and output a revised set of guides. <!-- [^8fa18-3] -->

The `pc oracle --interactive` mode keeps the full wiki loaded as persistent context across a REPL session for multi-turn Q&A and selective regeneration. <!-- [^8fa18-4] -->

A gap detection oracle pass sends the full wiki to a model and asks what topics a new developer would need that are not covered or underdefined. <!-- [^8fa18-5] -->
