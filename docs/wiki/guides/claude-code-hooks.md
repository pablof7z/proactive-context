---
title: Claude Code Hooks
slug: claude-code-hooks
topic: claude-code-hooks
summary: This guide documents the Claude Code hooks configuration used to inject proactive context into sessions
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-06-06
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:1fe0f5c6-3cb7-46b8-aa15-3175c834dffb
  - session:6e1a8676-e6b4-414c-b844-fbc3dbe437c0
---

# Claude Code Hooks

## Claude Code Hooks

This guide documents the Claude Code hooks configuration used to inject proactive context into sessions. Hooks invoke the installed binary at `~/.bin/proactive-context`.

## Hook Timeouts

- **UserPromptSubmit**: 30s timeout (inner inject timeout 25s).
- **SessionEnd**: 120s timeout.

## Hook Commands

All hooks invoke the installed binary at `~/.bin/proactive-context`. The SessionStart hook loads `open-questions.json`, drops any question whose `docs/wiki/<slug>.md` already exists, caps the list at 8 questions, and injects an `<open-questions>` block instructing the session to document them via `wiki_create`.

<!-- citations: [^1fe0f-d3059] [^6e1a8-df5b4] -->
