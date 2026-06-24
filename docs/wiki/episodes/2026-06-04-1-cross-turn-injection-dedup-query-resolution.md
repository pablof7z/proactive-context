---
type: episode-card
date: 2026-06-04
session: 32db6587-199d-4f6f-b185-0e71548dad65
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/32db6587-199d-4f6f-b185-0e71548dad65.jsonl
salience: product
status: active
subjects:
  - inject-cross-turn-dedup
  - inject-query-resolution
  - inject-ledger
supersedes: []
related_claims: []
source_lines:
  - 54-110
  - 500-620
  - 1004-1055
  - 1057-1107
  - 1137-1150
  - 1293-1300
captured_at: 2026-06-17T13:19:21Z
---

# Episode: Cross-turn injection dedup: query resolution + per-session ledger

## Prior State

Each inject call independently selected and compiled context. Anaphoric follow-ups ("that manifest") got poor retrieval, and facts already injected on prior turns were re-injected verbatim.

## Trigger

User asked what we send to the model at each inject path, revealing the gap; then live validation showed turn-2 re-injected facts the ledger already covered.

## Decision

Two mechanisms folded into the existing two LLM calls (zero added round-trips): (1) QUERY resolution in the SELECT gate — a new SELECT_RESOLVE_PREFIX makes the gate emit a leading QUERY: line decontextualizing follow-ups; parse_query_line extracts it, tolerant of model formatting. (2) Per-session JSONL ledger (~/.proactive-context/projects/<proj>/ledger/<session_id>.jsonl) tracks every prior briefing; fed to COMPILE as an ALREADY IN CONTEXT block instructing the model to surface only new facts (TITLE: none if nothing new). Gated by inject_resolve_query (default true) and inject_ledger_entries (default 8).

## Consequences

- Anaphoric prompts correctly resolved (e.g. "and where is that manifest stored?" → "Where is the deploy manifest file stored during a Lumen production deployment?")
- Dedup fires correctly with gemma models (turn 2 → TITLE: none, nothing re-injected)
- Initial dedup prompt wording was ignored by gemma; had to be strengthened with concrete worked example + post-sources reminder (commit 39727d1)
- deepseek-v4-flash ignores dedup instruction regardless of prompt strength — model obedience varies
- Ledger files are never pruned (open tail)
- COMPILE_PREAMBLE still claims "absolute file path" while labels are cwd-relative ./… (pre-existing, open tail)

## Open Tail

- Ledger pruning strategy not implemented
- COMPILE_PREAMBLE stale "absolute path" wording not yet fixed

## Evidence

- transcript lines 54-110
- transcript lines 500-620
- transcript lines 1004-1055
- transcript lines 1057-1107
- transcript lines 1137-1150
- transcript lines 1293-1300

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-04-1-cross-turn-injection-dedup-query-resolution.json`](transcripts/2026-06-04-1-cross-turn-injection-dedup-query-resolution.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-04-1-cross-turn-injection-dedup-query-resolution.json`](transcripts/raw/2026-06-04-1-cross-turn-injection-dedup-query-resolution.json)
