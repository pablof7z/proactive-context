---
type: episode-card
date: 2026-05-29
session: d00d68d4-f98d-46b7-be4d-51610d05bf3b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/d00d68d4-f98d-46b7-be4d-51610d05bf3b.jsonl
salience: architecture
status: active
subjects:
  - open-question-resolution
  - session-start-hook
  - autodoc
supersedes: []
related_claims: []
source_lines:
  - 3011-3098
  - 3134-3180
  - 3206-3217
captured_at: 2026-06-17T12:56:38Z
---

# Episode: Autodoc background agent replaced by SessionStart additionalContext injection

## Prior State

Undefined concepts detected at session end were resolved by spawning background autodoc agents — detached processes that grepped the codebase, called an LLM, and wrote low-confidence definition guides independently. Required: attempt markers, TTL caches, spawn/detach logic, codebase reading in a headless process.

## Trigger

User explicitly rejected the autodoc layer: "the autodoc thing is completely retarded and must be destroyed" — reasoning that Claude Code already has full codebase access and the background agent was a worse version of what the session naturally does.

## Decision

Deleted autodoc.rs entirely. SessionStart hook now simply reads open-questions.json, filters out nouns that already have a guide, and emits them as additionalContext in the hook response JSON. Claude sees the questions at session start and resolves them naturally using its existing tools. The wiki agent at session end then writes the definition guides through the existing capture pipeline.

## Consequences

- 280 lines deleted (autodoc.rs, Autodoc CLI subcommand, spawn/detach logic, attempt-marker caching, 7-day TTL)
- No background processes spawned at session start
- No attempt markers or TTL cache directories needed
- SessionStart hook exits immediately after emitting JSON — no async work
- Guide authoring quality improves because Claude has full codebase + tool access vs. a headless grep-then-LLM process
- Open questions cap at 8 per session to avoid context window bloat

## Open Tail

- Slug-mismatch between detected questions (e.g. `deploy-manifest`) and created guides (e.g. `deploy-manifest-edge-registry`) means exact-slug filtering can miss coverage — fuzzy or title-based matching may be needed

## Evidence

- transcript lines 3011-3098
- transcript lines 3134-3180
- transcript lines 3206-3217

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-autodoc-background-agent-replaced-by-sessionstart.json`](transcripts/2026-05-29-1-autodoc-background-agent-replaced-by-sessionstart.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-autodoc-background-agent-replaced-by-sessionstart.json`](transcripts/raw/2026-05-29-1-autodoc-background-agent-replaced-by-sessionstart.json)
