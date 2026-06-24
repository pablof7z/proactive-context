---
title: Agent Awareness
slug: agent-awareness
topic: agent-awareness
summary: The agent-awareness feature is removed from the project; awareness modules, subcommands, config fields, specs, and validation scripts must not exist
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-06-15
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:5465a19f-8d3b-45ea-8445-f8af794ce2c3
  - session:23f399e7-912c-4c7b-b960-4a1044e144ca
  - session:64c94ab4-45c5-4746-9d50-678dcfa6851c
---

# Agent Awareness


## Agent Awareness Removal

The agent-awareness feature is removed from the project; awareness modules, subcommands, config fields, specs, and validation scripts must not exist. The proactive-context config must not contain awareness fields (awareness_enabled, awareness_model, awareness_inject_min_interval_secs, awareness_expiry_secs). The codex config must not contain a PostToolUse hook block that runs `pc awareness`, nor any stale hash entry for that hook. Neither the opencode config nor the Claude settings file requires changes for awareness removal, as they contained no awareness hooks. The `awareness` wirings are dropped from `CLAUDE_WIRINGS` and `CODEX_WIRINGS`.

<!-- citations: [^9795b-10] [^64c94-1] -->
