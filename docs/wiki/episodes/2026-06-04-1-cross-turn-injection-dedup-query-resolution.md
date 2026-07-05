---
type: episode-card
date: 2026-06-04
session: 32db6587-199d-4f6f-b185-0e71548dad65
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/32db6587-199d-4f6f-b185-0e71548dad65.jsonl
salience: product
status: active
subjects:
  - inject-query-resolution
  - injection-ledger
  - cross-turn-dedup
supersedes: []
related_claims: []
source_lines:
  - 780-810
  - 1040-1057
  - 1137-1150
  - 1188-1193
captured_at: 2026-06-29T12:10:14Z
---

# Episode: Cross-turn injection dedup: query resolution + per-session ledger

## Prior State

Inject had no cross-turn deduplication — each turn independently compiled and injected briefings, re-injecting facts already visible in the assistant's context. No decontextualization of anaphoric follow-ups (e.g., 'and where is that manifest?'); the raw prompt went straight to both LLM calls.

## Trigger

User asked what exactly is sent to the model at each inject path. Investigation of the two-LLM-call pipeline (SELECT gate + COMPILE) revealed that follow-up turns would re-inject already-known facts, and that anaphoric prompts would reach the compiler without resolution.

## Decision

Two mechanisms folded into the existing two LLM calls (zero added round-trips): (1) SELECT_RESOLVE_PREFIX instructs the gate to emit a leading 'QUERY: <standalone question>' line, decontextualizing follow-ups and handling topic shifts; parse_query_line extracts it with tolerance for model formatting. (2) Per-session JSONL ledger at ~/.proactive-context/projects/<proj>/ledger/<session_id>.jsonl tracks every prior briefing; fed to the compiler as an 'ALREADY IN THE ASSISTANT'S CONTEXT — surface only NEW facts' block, with TITLE: none suppressing injection when nothing new. After validation showed the initial dedup wording was ignored by weak models, the prompt was strengthened with a concrete worked example and a post-sources reminder. Gated by inject_resolve_query (default true) and inject_ledger_entries (default 8, 0=off).

## Consequences

- 77 tests pass including parse_query_line formatting tolerance and ledger append→read→dedup round-trip
- End-to-end validated: anaphoric 'and where is that manifest file stored?' → resolved to 'Where is the deploy manifest file stored during a Lumen production deploy process?' and dedup correctly suppressed re-injection (briefing: NONE)
- Dedup obedience is model-dependent — deepseek-v4-flash ignores the strengthened instruction and re-injects; gemma models honor it
- Compile focal message is now the resolved query only — if the gate over-narrows a genuine topic pivot, compile synthesizes for the wrong question with no fallback
- Ledger files are never pruned (open non-blocker)
- COMPILE_PREAMBLE still says 'absolute file path' while source labels are cwd-relative ./… (pre-existing, unfixed)

## Open Tail

- Ledger files accumulate without pruning
- No fallback if gate over-narrows a topic pivot
- COMPILE_PREAMBLE stale path-label mismatch persists

## Evidence

- transcript lines 780-810
- transcript lines 1040-1057
- transcript lines 1137-1150
- transcript lines 1188-1193

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-04-1-cross-turn-injection-dedup-query-resolution.json`](transcripts/2026-06-04-1-cross-turn-injection-dedup-query-resolution.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-04-1-cross-turn-injection-dedup-query-resolution.json`](transcripts/raw/2026-06-04-1-cross-turn-injection-dedup-query-resolution.json)
