---
type: episode-card
date: 2026-05-29
session: 5465a19f-8d3b-45ea-8445-f8af794ce2c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5465a19f-8d3b-45ea-8445-f8af794ce2c3.jsonl
salience: product
status: superseded
subjects:
  - agent-awareness
  - multi-agent-coordination
  - awareness-hooks
  - pc-agents
supersedes: []
related_claims: []
source_lines:
  - 1-7
  - 191-199
  - 844-862
  - 982-982
  - 1156-1175
  - 1189-1228
  - 1557-1580
  - 1602-1660
  - 1799-1807
captured_at: 2026-06-17T13:01:48Z
---

# Episode: Ambient cross-agent awareness with delta injection and board viewer

## Prior State

No mechanism for concurrent Claude Code agents to know about each other's work; agents on the same git base were invisible to one another, and file-edit collisions were only discovered at commit time.

## Trigger

User requested design for cross-agent context injection: 'if the agent finds some problem in the code and there's another agent working at the same time we could inject some awareness' — envisioning brief status lines like 'Agent on branch X: Fixing auth bug [10s ago]' shared across worktrees of the same repo.

## Decision

Implemented a full ambient awareness system: (1) SQLite `agents.db` per repo room keyed by `git rev-parse --git-common-dir`, one row per session with distilled intent, backoff-distill schedule, and liveness tracking; (2) 4 hook points (UserPromptSubmit, PostToolUse, Stop, SessionEnd) that register, liveness-bump, backoff-distill, and mark-done respectively; (3) Delta injection on PostToolUse emitting only NEW/UPDATED/DONE peer changes via `additionalContext` with per-agent seen cursors and 30s throttle that holds (not drops) pending deltas; (4) `pc agents` on-demand standup board command added after user couldn't see the ephemeral-only deltas; (5) Distill model set to `ollama:glm-5.1:cloud` (~2s, no quota limit) after OpenRouter hit its daily 403 key limit.

## Consequences

- PostToolUse hook fires on every tool call in every project globally — cheap SQL check, LLM only via detached spawn.
- Another agent's commit (b9a996b) swept up uncommitted awareness wiring in main.rs/capture.rs, splitting the feature across two commits.
- The awareness board itself revealed the collision in real time — the feature that would have prevented the messy commit situation while shipping it.
- Distill summaries capture discovered scope beyond original task assignment (validated: 'Add Stripe webhooks' → distilled to 'Adding Stripe webhooks plus fixing missing signature verification on refund endpoint').
- SessionEnd provides clean 'done' signal; Stop is per-turn not per-session so cannot serve that role.
- Backoff distill schedule [60,150,300,600]s resets on new user prompts; final distill gated on backoff_index>0.

## Open Tail

- Docs (~14 files) still reference `proactive-context` — deferred because a peer agent was actively editing docs/wiki/.
- 4+ agents working on shared source files; awareness makes collisions visible but doesn't serialize commits — worktree-per-agent may be needed next.

## Evidence

- transcript lines 1-7
- transcript lines 191-199
- transcript lines 844-862
- transcript lines 982-982
- transcript lines 1156-1175
- transcript lines 1189-1228
- transcript lines 1557-1580
- transcript lines 1602-1660
- transcript lines 1799-1807

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-ambient-cross-agent-awareness-with-delta.json`](transcripts/2026-05-29-1-ambient-cross-agent-awareness-with-delta.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-ambient-cross-agent-awareness-with-delta.json`](transcripts/raw/2026-05-29-1-ambient-cross-agent-awareness-with-delta.json)
