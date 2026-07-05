---
type: noun-entry
slug: inject-subcommand
name: "inject (subcommand)"
origin: extracted
source_refs:
  - transcript:457-457
  - transcript:858-858
---

# inject (subcommand)

A Rust subcommand (replacing the deleted TypeScript ProactiveContext.hook.ts) invoked via the UserPromptSubmit hook; it compiles a relevance-filtered briefing for the current prompt — reading {prompt, cwd, session_id, transcript_path} JSON from stdin and writing a <system-reminder> block to stdout, never blocking the prompt.
