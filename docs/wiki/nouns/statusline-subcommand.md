---
type: noun-entry
slug: statusline-subcommand
name: "statusline (subcommand)"
origin: extracted
source_refs:
  - transcript:2073-2074
  - transcript:2098-2098
---

# statusline (subcommand)

A Rust subcommand invoked via Claude Code's statusLine.command: reads the statusLine stdin JSON (only session_id + cwd load-bearing), tails the last ~128 KB of events.jsonl filtered by session_id, renders one styled line, always exits 0 — sub-10ms, no LLM/network. Indicator format: ⬡ <inject title> · <N>w · <lat>s · Project Wiki: N guides.
