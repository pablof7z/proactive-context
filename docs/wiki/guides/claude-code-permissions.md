---
title: Claude Code Permission Rules
slug: claude-code-permissions
topic: claude-code-hooks
summary: In Claude Code permission allow rules, a glob is only valid in the tool-name position after a literal `mcp__<server>__` prefix (e.g
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:77e655bc-c95a-48c4-8277-b38fff616eac
---

# Claude Code Permission Rules

## MCP Allow-Rule Wildcard Syntax

In Claude Code permission allow rules, a glob is only valid in the tool-name position after a literal `mcp__<server>__` prefix (e.g. `mcp__github__*`). A bare `mcp__*` wildcard is not supported in allow rules and is silently skipped. Deny and ask permission rules accept wildcards anywhere, unlike allow rules which require a literal server prefix. <!-- [^77e65-d286d] -->

To auto-allow all tools from a given MCP server, list a per-server wildcard entry of the form `mcp__<server>__*` for each connected server. The trade-off is that a new entry must be added whenever a new MCP server is connected. <!-- [^77e65-d229e] -->

Removing an invalid `mcp__*` allow rule clears the `/doctor` warning with no behavior change, because the rule was already being silently skipped and MCP tools continue to prompt per-use as before. <!-- [^77e65-2ee4b] -->

## Editing Permission Settings

Editing `~/.claude/settings.json` triggers a permission prompt because `Edit(~/.claude/settings.json)` is in the ask list. <!-- [^77e65-0a38b] -->
