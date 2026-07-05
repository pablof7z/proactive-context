---
type: episode-card
date: 2026-05-29
session: 5465a19f-8d3b-45ea-8445-f8af794ce2c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5465a19f-8d3b-45ea-8445-f8af794ce2c3.jsonl
salience: product
status: active
subjects:
  - pc-agents
  - standup-board
  - on-demand-viewer
supersedes: []
related_claims: []
source_lines:
  - 1557-1580
  - 1603-1679
captured_at: 2026-06-29T11:37:03Z
---

# Episode: On-demand `pc agents` standup board added after ephemeral-only design felt broken

## Prior State

Awareness surfaced only as ephemeral fire-once deltas (NEW/UPDATED/DONE) injected after tool calls, throttled to once/30s, scrolling away. No way to pull up current state on demand.

## Trigger

User reported 'I don't think it's working — I don't see anything cross-session.' Investigation showed the feature WAS working (deltas were firing, agents.db had rows), but the ephemeral-only surfacing meant the user couldn't perceive it.

## Decision

Added the `Agents` subcommand (rendered as `pc agents`) — an on-demand snapshot of all concurrent agents showing session id, status (active/done/expired), [branch, age], and distilled intent. `--all` flag shows expired agents (>1h idle). Worktree path prints on sub-line when agent is in a different worktree.

## Consequences

- Two complementary surfacing paths now exist: ephemeral deltas (PostToolUse) and on-demand board (`pc agents`)
- The board reads live from agents.db, so it reflects real-time state including the calling session's own current distilled intent
- User can now verify the feature is working at any time without waiting for a delta to fire

## Open Tail

*(none)*

## Evidence

- transcript lines 1557-1580
- transcript lines 1603-1679

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-3-on-demand-pc-agents-standup-board.json`](transcripts/2026-05-29-3-on-demand-pc-agents-standup-board.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-3-on-demand-pc-agents-standup-board.json`](transcripts/raw/2026-05-29-3-on-demand-pc-agents-standup-board.json)
