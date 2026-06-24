---
title: TENEX Ingestion
slug: tenex-ingestion
topic: conversation-ingestion
summary: The archeologist subcommand auto-detects and loads conversations from all supported sources (Claude Code, TENEX, Codex, and opencode) simultaneously when run, r
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-06-10
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:94d06a3c-7fd2-47ef-8022-6f63e5793f71
  - session:8fa18555-86b6-492d-9b13-1865774df99c
  - session:08870c09-c42d-44bf-9272-6f306cee3b52
---

# TENEX Ingestion

## TENEX Ingestion

The archeologist subcommand auto-detects and loads conversations from all supported sources (Claude Code, TENEX, Codex, and opencode) simultaneously when run, rather than requiring individual opt-in flags. Each conversation source is displayed with its source name (e.g. [tenex], [codex], [opencode]) as a tag in the selection TUI. Sources whose paths are not present on the system are skipped silently during auto-detection. TENEX entries route to the same wiki as co-located Claude Code sessions on the same path. (Previously: A --tenex flag enabled TENEX project scanning; TENEX projects merged into the picker alongside Claude Code sessions.)

<!-- citations: [^94d06-1] [^08870-3] -->
## Configuration

TENEX project base directory is read from ~/.tenex/config.json (projectsBase field), not scanned from common directories. The local cwd for a TENEX project is resolved as projectsBase/<slug> from config; projects where that directory does not exist are skipped. The user's whitelisted pubkey is read from whitelistedPubkeys[0] in ~/.tenex/config.json. <!-- [^94d06-2] -->

## Filtering and Deduplication

Only conversations where the user was a participant (user's pubkey appears in at least one message) are imported; agent-to-agent communication is excluded. All conversations are importable regardless of message count. Consecutive identical assistant messages are deduplicated to remove TENEX retry chatter. The content field is used for both user and assistant messages; human_readable is never populated in practice. (Previously: Conversations with fewer than 3 messages were skipped.)

<!-- citations: [^94d06-3] [^8fa18-6] -->
## Session Mapping and Synthesis

The Nostr conversation_id is used directly as the pc session_id, making importing idempotent and avoiding duplication with TENEX's own hooks-based capture. Each conversation is pre-synthesized to a flat JSONL temp file with a [TENEX project: ..., conversation: "..."] preamble on the first user turn. The TempDir for synthesized JSONL files is held alive for the entire run duration and auto-cleaned on drop. <!-- [^94d06-4] -->

## Codex Ingestion

Codex history scanning reads ~/.codex/sessions/ and ~/.codex/archived_sessions/ recursively for rollout-*.jsonl files, which use the native session_meta + response_item wire format and require no synthesis. Legacy Codex rollout-*.json files (which lack a cwd field) are counted and skipped with a message during scanning. <!-- [^08870-4] -->
