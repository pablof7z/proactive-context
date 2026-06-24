---
title: OpenCode Integration
slug: opencode-integration
topic: agent-system
summary: opencode's plugin system is viable for hosting pc's core flows (inject and capture), but the integration primitives differ from Claude Code's approach
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-07
updated: 2026-06-10
verified: 2026-06-07
compiled-from: conversation
sources:
  - session:29c495dd-0ac9-4ccc-8914-7be9fe6e703f
  - session:08870c09-c42d-44bf-9272-6f306cee3b52
---

# OpenCode Integration

## Integration Approach

opencode's plugin system is viable for hosting pc's core flows (inject and capture), but the integration primitives differ from Claude Code's approach. opencode hooks are JS/TS functions in a plugin module, not shell commands, so pc (a Rust binary) requires a thin JS plugin shim that shells out to the `pc` binary. The shim should live in `.opencode/plugins/` or `~/.config/opencode/plugins/` and use `ctx.$` (Bun shell) to exec the `pc` binary, passing prompts/messages on stdin and splicing `pc`'s stdout into the messages array. <!-- [^29c49-2] -->


Opencode history scanning reads ~/.local/share/opencode/opencode.db (SQLite), queries sessions by directory, and synthesizes flat JSONL from the message and part tables using the same preamble pattern as TENEX. <!-- [^08870-2] -->
## Hook Mappings

The inject flow (prompt-aware context injection) maps to opencode's `experimental.chat.messages.transform` hook, which allows reading the latest user message and pushing a briefing into the messages array. The capture flow (distilling a transcript to wiki after a turn settles) maps to opencode's `event` hook filtering on `session.idle`. Session lifecycle hooks map to `event` → `session.created`, and `ctx.client` SDK or direct on-disk session storage reads for transcript access. <!-- [^29c49-3] -->

## Awareness Deltas

opencode's `tool.execute.after` hook can observe tool calls for awareness deltas but cannot inject context mid-turn, so awareness deltas degrade to next-turn injection via `messages.transform` instead. <!-- [^29c49-4] -->

## Stability Caveats

The injection path depends on `experimental.*` hooks in opencode, and the exact hook name/signature should be expected to churn until a dedicated pre-inference hook (`chat.request.before`) lands non-experimentally. <!-- [^29c49-5] -->

## Tracking

A GitHub issue exists documenting the research and proposed spike for hooking pc into opencode via its plugin system. <!-- [^29c49-6] -->
