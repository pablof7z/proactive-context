---
title: Debug Transcript Command
slug: debug-transcript-command
topic: cli-daemon
summary: The `pc debug transcript --all` command resolves the project root from the current working directory, scans `~/.claude/projects/` for all sessions whose cwd map
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:b38015dd-d2aa-4e83-8671-40346633a176
---

# Debug Transcript Command

## Debug Transcript Command

The `pc debug transcript --all` command resolves the project root from the current working directory, scans `~/.claude/projects/` for all sessions whose cwd maps to that root, and prints each numbered transcript in mtime order. <!-- [^b3801-4] -->
