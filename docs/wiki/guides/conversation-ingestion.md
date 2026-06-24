---
title: Conversation Ingestion
slug: conversation-ingestion
topic: data-persistence
summary: Conversation logs from Claude Code are stored in `~/.claude/projects/*/`
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-06-19
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:0bf0fe1c-fbf5-497e-b286-e364266abf05
  - session:5e3f025e-badc-4f34-ab5e-757ee942bf2c
  - session:8eff6130-2e37-410c-968c-a73ff4acc88c
---

# Conversation Ingestion

## Conversation Log Storage

Conversation logs from Claude Code are stored in `~/.claude/projects/*/`. A watcher on that directory can pick them up automatically.

Conversations are stored as a separate corpus from markdown files, using a separate `index.db` with a different chunking strategy and metadata (timestamp, project, session ID).

NMP claude transcripts (~228 sessions) and codex rollouts (538 matched by cwd) were rsynced to `pablo@157.180.102.242` for continuing the nostr-multi-platform regen there, using `tar -czf - -T` piping over SSH after macOS `openrsync` incompatibilities with `--files-from` and `--info=stats2`.

Transferred session files encode Mac paths (`/Users/pablofernandez/...`); path adaptation (renaming claude dirs and rewriting `cwd` fields inside JSONL) is needed for the archeologist to route correctly on the remote Linux machine.

<!-- citations: [^0bf0f-1] [^0bf0f-2] [^5e3f0-1] [^8eff6-40] [^8eff6-74] -->
## Conversation Reconstruction

Episode cards carry a Conversation section reconstructed from the session JSONL by scanning each JSONL entry, keeping only entries whose effective message role is `user` or `assistant`, extracting speakable text blocks, and serializing the resulting dialogue as `[[role, text], ...]` transcript JSON. The section appears after Evidence in the episode card and links to the cleaned and raw transcript JSON files rather than embedding the dialogue inline.

Genuine user speech is distinguished from infrastructure content by both metadata and content shape: newer transcripts use `promptSource: "typed"` for terminal input and `promptSource: "queued"` for queued input, while injected context is `promptSource: "system"`; older transcripts without `promptSource` fall back to dropping XML-ish injected blocks that begin with `<`. Tool results, tool calls, and thinking blocks are not dialogue because only text-like block types (`text` and `input_text` for user entries, `text` and `output_text` for assistant entries) are kept.

Tool results can appear under the user role because assistant runtimes feed tool output back to the model as user-role transcript entries, but they are structurally distinguishable from human-authored text when the content blocks are typed as `tool_result` rather than `text`/`input_text`. System-reminder blocks, hook context, agent inbox dumps, command wrappers, task notices, and command output may also ride the user channel; episode dialogue should filter them unless they are explicit human-authored text.

Consecutive agent turns collapse to only the last spoken message because intermediate assistant chatter in a run is not what the user replies to; the final assistant message before the next user turn is the stable conversational state. This collapse applies only to speakable assistant text and ignores thinking/tool-use blocks.

Note: tenex-edge fabric inbox messages from other agents are currently stamped `promptSource: "typed"`, which makes them structurally indistinguishable from genuine human prompts at this layer. They remain a known filtering bug and can leak into episode dialogue until tenex-edge stamps them with a non-human source or an additional machine-authorship marker.

The episode conversation undergoes an LLM cleanup pass (default-on via the `clean_episode_dialogue` config flag) that keeps user words verbatim but strips pasted content (terminal output, logs, stack traces, file/code dumps), and abbreviates agent replies to 1–2 sentences. <!-- [^5e3f0-2] -->

## Transcript Storage

Episode transcripts are stored as separate JSON files rather than embedded in the card body, in the format `[["user", "..."], ["assistant", "..."], ...]`. Transcript filenames match the slugged title of their corresponding card file exactly (not a UUID or session ID).

Two transcript JSON files are written per episode card: a cleaned transcript at `episodes/transcripts/<slug>.json` (LLM-summarized agent replies, pasted content stripped) and a raw transcript at `episodes/transcripts/raw/<slug>.json` (full agent replies, pasted user content kept, but still with system-injected content stripped).

The episode card's Conversation section links to both the cleaned and raw transcript JSON files rather than inlining the dialogue.

Transcript JSON files are never indexed or injected into the LLM context because the daemon's file watcher and episode catalog scanner only process `.md` files. <!-- [^5e3f0-3] -->
