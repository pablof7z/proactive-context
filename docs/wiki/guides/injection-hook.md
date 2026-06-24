---
title: Injection Hook
slug: injection-hook
topic: agent-system
summary: A Claude Code UserPromptSubmit hook runs a proactive-context query against the current prompt and prepends the top hits as a system-level context block
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-06-15
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:0bf0fe1c-fbf5-497e-b286-e364266abf05
  - session:b6eb3345-88d9-49ce-95bd-06f7851639c8
  - session:bede7c40-d729-4df6-8e97-f25dca6ce66a
  - session:ed37c932-17ed-4003-935e-d232e9195c59
  - session:d00d68d4-f98d-46b7-be4d-51610d05bf3b
  - session:29c495dd-0ac9-4ccc-8914-7be9fe6e703f
  - session:64c94ab4-45c5-4746-9d50-678dcfa6851c
---

# Injection Hook

## Injection Hook

A Claude Code UserPromptSubmit hook runs a proactive-context query against the current prompt and prepends the top hits as a system-level context block. The hook lives in global settings (not project-level) so it fires in any project directory. Stale lessons risk being confidently injected as truth, conversation logs risk cross-contaminating sensitive project context, and the injection hook adds latency to every prompt. (Previously: the project-level settings.json with the echo hook was removed because it was redundant with the global settings hook.)

The `experimental.chat.system.transform` hook must not be used for prompt-aware injection because it does not include the user's message text, and a feature request to add it was closed as not-planned. The `messages.transform` approach for prompt-aware injection is proven by the `opencode-dynamic-context-pruning` plugin, which performs production message-array rewriting. <!-- [^29c49-1] -->

Hook-based invocations use a `hook` subcommand (e.g. `pc hook capture` instead of `pc capture`). The `hook` subcommand accepts a `--harness <claude|opencode|codex>` flag to disambiguate harness-specific behavior. The `Statusline` hook action does not take a `--harness` flag. <!-- [^64c94-3] -->

<!-- citations: [^0bf0f-3] [^b6eb3-1] -->

## Inject Command Interface

The `inject` command receives `{ prompt, cwd, session_id, transcript_path }` via stdin and reads the full transcript from the path on disk. Inject uses both the current user prompt and the last 6 turns of conversation history (capped at 2000 chars tail) to form a `recent` context string. The `recent` context enriches the vector retrieval query as `recent + "\n\n" + prompt`, and is fed as a preamble to both the select and compile LLM models. The inject command uses a two-step LLM pipeline: a fast navigation model that selects relevant guides, followed by a strong compile model that extracts verbatim sections for the briefing.

<!-- citations: [^b6eb3-2] [^ed37c-9] -->
## Verbose Mode

The `inject` subcommand has a `--verbose` flag that, instead of silently injecting context, outputs JSON containing a `systemMessage` field (visible in the Claude Code UI) showing timing, hit count, guides read, full briefing text, and reasons for any skip/short-circuit/fallback/timeout. The non-verbose `inject` path outputs a plain `<system-reminder>` block to stdout and is unchanged by the verbose feature. <!-- [^b6eb3-3] -->

## No-Database Project Detection

When the inject hook runs in a project with no existing database, it scans for .md/.markdown files using WalkBuilder (respecting .gitignore) and branches into three outcomes. If the scan finds 5 or fewer indexable files, the hook silently does nothing (no indexing, no prompt). If the scan finds more than 5 files with a total LOC of 5000 or less, the hook silently calls daemonize() to fork a background daemon and continues immediately. If the scan finds more than 5 files with a total LOC exceeding 5000, the hook prints the candidate list (up to 100 files) and a prompt instructing Claude Code to ask the user whether they want to gather knowledge about the project, and suggests running `proactive-context init` if the user says yes. <!-- [^bede7-1] -->

## SessionStart Hook

The `SessionStart` hook is available in Claude Code and fires before the user's first prompt, with source variants including startup, resume, clear, and compact. <!-- [^d00d6-6] -->
