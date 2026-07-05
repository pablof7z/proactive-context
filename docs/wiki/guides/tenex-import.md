---
title: TENEX Import
slug: tenex-import
topic: capture-pipeline
summary: TENEX project discovery reads `~/.tenex/config.json` for the projects base directory and the user's whitelisted pubkey, rather than scanning for projects.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:94d06a3c-7fd2-47ef-8022-6f63e5793f71
---

# TENEX Import

## Project Discovery

TENEX project discovery reads `~/.tenex/config.json` for the projects base directory and the user's whitelisted pubkey, rather than scanning for projects. <!-- [^94d06-f0773] -->

`~/.tenex/projects/<slug>/event.json` is a Nostr kind:31933 event containing a `repo` git-remote tag and a `title` tag. <!-- [^94d06-4c6d6] -->

`~/.tenex/projects/<slug>/conversation.db` is a SQLite database with a `conversations` table (id, title, created_at) and a `messages` table (role, author_pubkey, content, timestamp, human_readable) capturing Nostr conversations. <!-- [^94d06-2c883] -->

TENEX conversation.db messages use the `content` field directly for both user and assistant roles because the `human_readable` field is never populated. <!-- [^94d06-5116f] -->

## Import Pipeline

The TENEX conversation_id is used directly as the pc session_id, making imports idempotent and avoiding duplication with TENEX's own pc hooks. <!-- [^94d06-40dff] -->

Each TENEX conversation is pre-synthesized to a flat JSONL temp file with a `[TENEX project: ..., conversation: "..."]` preamble on the first user turn, fed through the existing capture pipeline. <!-- [^94d06-bed85] -->

## UI & Merging

TENEX-originated entries are labeled `[tenex]` in the project picker and merge into the same wiki as co-located Claude Code sessions on the same path. <!-- [^94d06-716ce] -->
