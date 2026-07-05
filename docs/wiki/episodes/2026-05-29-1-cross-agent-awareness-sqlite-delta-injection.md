---
type: episode-card
date: 2026-05-29
session: 5465a19f-8d3b-45ea-8445-f8af794ce2c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5465a19f-8d3b-45ea-8445-f8af794ce2c3.jsonl
salience: architecture
status: active
subjects:
  - agent-awareness
  - sqlite-storage
  - delta-injection
  - transcript-distillation
supersedes: []
related_claims: []
source_lines:
  - 1-7
  - 191-199
  - 846-858
  - 1308-1332
captured_at: 2026-06-29T11:37:03Z
---

# Episode: Cross-agent awareness: SQLite delta-injection architecture adopted

## Prior State

No mechanism existed for concurrent Claude Code agents to be aware of each other's work. Agents working in parallel on the same git base had no shared context about what peers were doing.

## Trigger

User directive to design a system that injects awareness of other concurrent agents into an agent's context, keyed by shared git base across worktrees.

## Decision

Adopted a per-repo SQLite `agents.db` (WAL mode) storing per-agent rows (session_id, worktree, branch, intent_summary, liveness timestamps, backoff state). Agents distill their intent from their own transcript via a detached worker reusing capture's setsid spawn + call_model_blocking. Deltas (NEW/UPDATED/DONE) are injected via PostToolUse additionalContext JSON, with per-agent seen-cursors, 30s throttle that holds (not drops) deltas, self-exclusion, and 1h expiry. Four hooks wired: UserPromptSubmit, PostToolUse, Stop, SessionEnd.

## Consequences

- agents.db is per-repo-room (project_context_dir), shared across worktrees of the same git base
- Column-scoped updates ensure detached distill workers never clobber liveness bumps from hooks
- Backoff distill schedule [60,150,300,600]s resets on UserPromptSubmit/Stop; final distill on Stop gated by backoff_index>0
- PostToolUse hook fires on every tool call in every project globally — a latency/cost consideration for all sessions
- awareness_enabled config flag provides a global kill-switch for all four hooks
- 12/12 offline validation suite driving real hook stdin/stdout contract for two simulated agents

## Open Tail

- PostToolUse hook is active globally on every project — if it becomes a problem, set awareness_enabled: false or restore settings.json.bak-awareness

## Evidence

- transcript lines 1-7
- transcript lines 191-199
- transcript lines 846-858
- transcript lines 1308-1332

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-cross-agent-awareness-sqlite-delta-injection.json`](transcripts/2026-05-29-1-cross-agent-awareness-sqlite-delta-injection.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-cross-agent-awareness-sqlite-delta-injection.json`](transcripts/raw/2026-05-29-1-cross-agent-awareness-sqlite-delta-injection.json)
