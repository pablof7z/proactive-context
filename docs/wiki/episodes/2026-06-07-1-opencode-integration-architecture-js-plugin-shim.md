---
type: episode-card
date: 2026-06-07
session: 29c495dd-0ac9-4ccc-8914-7be9fe6e703f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/29c495dd-0ac9-4ccc-8914-7be9fe6e703f.jsonl
salience: architecture
status: active
subjects:
  - opencode-integration
  - pc-hook-mapping
supersedes: []
related_claims: []
source_lines:
  - 178-206
captured_at: 2026-06-17T13:48:10Z
---

# Episode: opencode integration architecture: JS plugin shim with messages.transform

## Prior State

pc integrates with Claude Code via shell-command hooks registered in settings.json (UserPromptSubmit→inject, Stop→capture, PostToolUse→awareness deltas). No integration path existed for opencode; unknown whether opencode's plugin system could support pc's core flows.

## Trigger

User requested research into whether opencode has hooks usable for pc integration. Investigation revealed opencode's JS/TS plugin system with message-transform, event, and system-transform hooks — plus GitHub issues confirming system.transform cannot see the user prompt.

## Decision

opencode is hookable but requires a different integration shape: (1) Use `experimental.chat.messages.transform` for inject — NOT `experimental.chat.system.transform` (which cannot see user message text, per closed-not-planned issues #27401 and #17637). (2) Use `session.idle` event hook for capture. (3) Bridge via a JS plugin shim that uses `ctx.$` (Bun shell) to exec the `pc` binary, since opencode plugins are JS/TS functions, not shell commands. (4) Mid-turn awareness deltas degrade to next-turn injection (opencode's `tool.execute.after` can observe but cannot inject context).

## Consequences

- New target platform for pc beyond Claude Code and Codex
- Inject hook depends on experimental.chat.messages.transform — expect API name/signature churn until pre-inference hook discussion (#21240/#19425) lands non-experimentally
- No mid-turn system-reminder injection equivalent; awareness deltas must be folded into the next messages.transform call
- Requires shipping a JS plugin shim as a new artifact in the pc project
- system.transform is permanently excluded for prompt-aware injection (maintainer closed as not-planned)

## Open Tail

- Prototype the JS plugin shim (.opencode/plugins/ that execs pc inject/pc capture)
- Monitor opencode experimental API stabilization for messages.transform
- Decide whether the opencode plugin shim lives in the pc repo or as a separate package

## Evidence

- transcript lines 178-206

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-07-1-opencode-integration-architecture-js-plugin-shim.json`](transcripts/2026-06-07-1-opencode-integration-architecture-js-plugin-shim.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-07-1-opencode-integration-architecture-js-plugin-shim.json`](transcripts/raw/2026-06-07-1-opencode-integration-architecture-js-plugin-shim.json)
