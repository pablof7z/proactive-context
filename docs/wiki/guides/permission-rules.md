---
title: Permission Rules
slug: permission-rules
topic: permissions
summary: Allow rules in `permissions.allow` require a literal `mcp__<server>__` prefix; globs are permitted only in the tool position after that prefix (e.g
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

## Permission Rules

Allow rules in `permissions.allow` require a literal `mcp__<server>__` prefix; globs are permitted only in the tool position after that prefix (e.g. `mcp__github__*`), and bare wildcards like `mcp__*` are invalid and silently skipped. Deny and ask rules accept wildcards anywhere, unlike allow rules. When auto-allowing MCP tools, the supported form is per-server: `mcp__<server>__*`. The invalid `mcp__*` rule was removed from `permissions.allow`, leaving MCP tools prompting per-use with no behavior change since the rule was already being skipped. <!-- [^77e65-1] -->
