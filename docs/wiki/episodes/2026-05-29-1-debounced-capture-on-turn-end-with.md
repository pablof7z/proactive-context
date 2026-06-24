---
type: episode-card
date: 2026-05-29
session: acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/acecaa67-07c0-48fa-bd2d-4dc0c2c42c6a.jsonl
salience: product
status: superseded
subjects:
  - capture-pipeline
  - stop-hook
  - triage-gate
  - debounce
supersedes: []
related_claims: []
source_lines:
  - 1-3
  - 320-646
captured_at: 2026-06-17T12:21:35Z
---

# Episode: Debounced capture on turn-end with dedup and triage gate

## Prior State

Capture only ran on SessionEnd hook — no way to extract learnings mid-session. Every capture invoked the expensive thinking model regardless of transcript signal.

## Trigger

User requested: (1) capture transcript when a Claude Code turn ends (Stop hook) with 5-minute debounce in case the user re-engages, (2) dedup so SessionEnd skips already-captured transcripts, (3) fast-model triage to skip trivial conversations without calling the thinking model.

## Decision

Added `capture --in <secs>` for Stop hook that writes a pending file, kills any prior debounce process, forks a background `capture --deferred` via setsid, sleeps the debounce interval, then runs capture only if still the winner. SessionEnd checks a marker file — if the debounce already captured N exchanges and current count ≤ N, it skips. Haiku triage gate (default `anthropic/claude-haiku-4-5`) decides YES/NO before invoking Sonnet distillation.

## Consequences

- Capture now fires on both Stop (debounced) and SessionEnd (immediate, with dedup marker)
- Trivial sessions cost only a Haiku call, not two Sonnet calls
- Background debounce process must survive hook exit (setsid)
- Marker dedup is by exchange count (extent), not just session_id — prevents stale-capture races
- Stop hook stdin format still unverified empirically

## Open Tail

- Stop hook stdin shape and setsid survival need empirical testing before trust
- rig-core OpenRouter tool-loop compatibility unverified

## Evidence

- transcript lines 1-3
- transcript lines 320-646

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-debounced-capture-on-turn-end-with.json`](transcripts/2026-05-29-1-debounced-capture-on-turn-end-with.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-debounced-capture-on-turn-end-with.json`](transcripts/raw/2026-05-29-1-debounced-capture-on-turn-end-with.json)
