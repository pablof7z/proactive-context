---
type: episode-card
date: 2026-06-17
session: 54cada63-dcb1-4088-9838-22639779ca06
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/54cada63-dcb1-4088-9838-22639779ca06.jsonl
salience: product
status: active
subjects:
  - archeologist-token-tally
  - run-counters-usage
supersedes:
  - 2026-06-16-1-archeologist-live-token-cost-tally-replaces
related_claims: []
source_lines:
  - 1-6
  - 385-400
  - 887-896
  - 915-942
  - 989-1004
captured_at: 2026-06-17T21:43:33Z
---

# Episode: Archeologist gains live token/cost tally in TUI and line-log

## Prior State

Archeologist's RunCounters only read a nested `usage.*` shape for token accounting, which was stale for real `llm.response` events. No cost tracking existed. The TUI header showed only estimated cost, and line-log mode had no per-session token/cost information at all.

## Trigger

User explicitly requested: 'to have pc archeologist show me a running tally of all the tokens we are using'

## Decision

Added live token/cost tally to both TUI and line-log modes. RunCounters now reads both the flat `prompt_tokens`/`completion_tokens`/`cost_usd` keys from `llm.response` events and the legacy nested `usage.*` shape. A `cost_usd` accumulator was added. The TUI header shows estimated vs actual cost color-coded (green ≤ low estimate, yellow ≤ high estimate, red > high estimate). Line-log prints a running tally after each session and full totals in the final summary.

## Consequences

- Users can now monitor actual token spend in real-time during bulk historical capture runs
- Dual-shape usage reading preserves backward compatibility with both flat llm.response keys and older nested usage payloads
- Cost color-coding provides at-a-glance budget health during long runs
- The pre-existing lazy-picker refactor and capture.rs hook-subcommand fix were committed alongside this change in the same commit

## Open Tail

*(none)*

## Evidence

- transcript lines 1-6
- transcript lines 385-400
- transcript lines 887-896
- transcript lines 915-942
- transcript lines 989-1004

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-17-1-archeologist-gains-live-token-cost-tally.json`](transcripts/2026-06-17-1-archeologist-gains-live-token-cost-tally.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-17-1-archeologist-gains-live-token-cost-tally.json`](transcripts/raw/2026-06-17-1-archeologist-gains-live-token-cost-tally.json)
