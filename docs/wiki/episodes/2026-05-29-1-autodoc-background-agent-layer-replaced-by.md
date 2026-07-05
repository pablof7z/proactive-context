---
type: episode-card
date: 2026-05-29
session: d00d68d4-f98d-46b7-be4d-51610d05bf3b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/d00d68d4-f98d-46b7-be4d-51610d05bf3b.jsonl
salience: reversal
status: active
subjects:
  - session-start-hook
  - autodoc-removal
  - open-questions-detection
supersedes: []
related_claims: []
source_lines:
  - 3011-3033
  - 3034-3050
  - 3134-3140
  - 3206-3216
captured_at: 2026-06-29T11:28:14Z
---

# Episode: Autodoc background-agent layer replaced by SessionStart additionalContext injection

## Prior State

No mechanism existed for surfacing undefined concepts from prior sessions at session start. The initial design created an autodoc subcommand: a detached background agent that grep'd the codebase, made a standalone LLM call, wrote low-confidence definition guides, and managed attempt markers with a 7-day TTL cache.

## Trigger

User explicitly rejected the autodoc approach: 'the autodoc thing is completely retarded and must be destroyed' — reasoning that Claude Code already has full codebase access and tools, making a background grep→LLM→write process a strictly worse version of what the session itself does natively.

## Decision

Deleted the entire autodoc layer (src/autodoc.rs, Autodoc command, attempt-marker caching, background agent spawning). SessionStart hook now reads open-questions.json, filters out questions whose slug already has a wiki guide, and emits up to 8 remaining questions as additionalContext JSON. Claude answers them naturally during the session using existing wiki tools; capture's wiki agent picks up the answers at session end.

## Consequences

- 331 lines deleted (autodoc.rs + session_start.rs rewrite); net -280 lines
- No background processes, no setsid spawning, no attempt markers, no TTL caches
- Open-question resolution is now the session's responsibility via wiki_create, not a background process
- SessionStart hook wired into ~/.claude/settings.json with SessionEnd and UserPromptSubmit
- Questions capped at 8 per session to avoid overwhelming the context window

## Open Tail

- Slug mismatch between detection (e.g. 'deploy-manifest') and wiki agent guide creation (e.g. 'deploy-manifest-edge-registry') means some answered questions stay open because the filter checks exact slug equality

## Evidence

- transcript lines 3011-3033
- transcript lines 3034-3050
- transcript lines 3134-3140
- transcript lines 3206-3216

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-autodoc-background-agent-layer-replaced-by.json`](transcripts/2026-05-29-1-autodoc-background-agent-layer-replaced-by.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-autodoc-background-agent-layer-replaced-by.json`](transcripts/raw/2026-05-29-1-autodoc-background-agent-layer-replaced-by.json)
