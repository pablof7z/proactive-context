---
type: episode-card
date: 2026-06-16
session: 54cada63-dcb1-4088-9838-22639779ca06
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/54cada63-dcb1-4088-9838-22639779ca06.jsonl
salience: product
status: superseded
subjects:
  - archeologist-token-tally
  - run-counters-usage-shapes
supersedes: []
related_claims: []
source_lines:
  - 1-6
  - 940-950
  - 886-900
  - 989-1004
captured_at: 2026-06-17T14:15:28Z
---

# Episode: Archeologist live token/cost tally replaces estimate-only display

## Prior State

Archeologist's TUI showed only estimated cost ranges; RunCounters accumulated token counts only from the nested `usage.*` payload shape, silently dropping real `llm.response` events that emit flat `prompt_tokens`/`completion_tokens`/`cost_usd` keys. No actual-spend field existed. Line-log mode had no token or cost output at all.

## Trigger

User requested a running tally of tokens used during archeologist runs. Implementation revealed that the existing `apply()` method ignored the flat-key shape, so real usage data was never counted.

## Decision

Added `cost_usd` accumulator to RunCounters; fixed `apply()` to read both nested `usage.*` and flat `prompt_tokens`/`completion_tokens`/`cost_usd` shapes. TUI header now shows `Cost est ~$lo-$hi actual ~$X.XX tokens X in / Y out` with actual-cost color-coded green/yellow/red against the estimate. Line-log mode prints per-session token/cost tallies and a final summary.

## Consequences

- Users can now observe real LLM spend in real time and compare against pre-run estimates
- Previously invisible token accounting gap (flat-key payloads silently dropped) is now closed
- TUI cost-line color semantics: green if actual ≤ low estimate, yellow if ≤ high estimate, red if exceeding high estimate
- Both line-log and TUI modes now converge on the same RunCounters accumulator, so tallies are consistent

## Open Tail

- The pre-existing lazy-picker refactor (scan_all_projects, run_lazy_picker) is interleaved in the same file but uncommitted and unowned by this session — commit strategy still pending user direction

## Evidence

- transcript lines 1-6
- transcript lines 940-950
- transcript lines 886-900
- transcript lines 989-1004

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-16-1-archeologist-live-token-cost-tally-replaces.json`](transcripts/2026-06-16-1-archeologist-live-token-cost-tally-replaces.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-16-1-archeologist-live-token-cost-tally-replaces.json`](transcripts/raw/2026-06-16-1-archeologist-live-token-cost-tally-replaces.json)
