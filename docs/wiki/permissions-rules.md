---
title: Permission Rules
slug: permissions-rules
topic: permissions-rules
summary: In permissions.allow rules, a glob wildcard is only valid in the tool position after a literal `mcp__<server>__` prefix
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

# Permission Rules

## Wildcard Placement Rules

In permissions.allow rules, a glob wildcard is only valid in the tool position after a literal `mcp__<server>__` prefix. Deny and ask permission rules accept wildcards anywhere in the pattern. <!-- [^77e65-2] -->

## Supported Auto-Allow Pattern

The supported form to auto-allow all tools from a specific MCP server is `mcp__<server>__*`. <!-- [^77e65-3] -->

## Invalid Rule Handling

An invalid permission rule in permissions.allow is silently skipped with no effect on behavior. Removing an invalid `mcp__*` rule from permissions.allow changes nothing about current behavior and only clears the warning. <!-- [^77e65-4] -->

## Settings File Permission Behavior

Editing `~/.claude/settings.json` triggers a permission prompt when `Edit(~/.claude/settings.json)` is in the ask list. <!-- [^77e65-5] -->
