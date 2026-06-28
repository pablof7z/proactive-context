---
type: episode-card
date: 2026-06-28
session: 5e3eb16d-da30-4e4d-938f-4d3e508fd24d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/5e3eb16d-da30-4e4d-938f-4d3e508fd24d.jsonl
salience: architecture
status: active
subjects:
  - usage-accounting
  - module-organization
  - recall-coupling
supersedes: []
related_claims: []
source_lines:
  - 1-207
  - 209-209
  - 532-1020
captured_at: 2026-06-28T11:10:56Z
---

# Episode: Move Usage from recall subsystem to top-level global module

## Prior State

Usage struct defined in recall::usage, tightly coupled to recall subsystem. Only recall and claude_sidecar could access it. Cache tokens only tracked in recall-repl path; silently dropped by openrouter.rs parse_usage, invisible to capture pipeline and archeologist.

## Trigger

User directive: 'usage shouldn't have been coupled to recall -- it should be global to ANY and ALL llm calls! including claude-cli'. Triggered by discovery that cache accounting was missing from multiple LLM call paths.

## Decision

Move Usage and Ledger from recall::usage to top-level crate::usage. Update all LLM call paths (openrouter, claude_cli, claude_sidecar, recall) to import from crate::usage. Add cached_tokens parsing to openrouter.rs (was being silently dropped). Simplify Usage signature: cost: Option<f64> replaces (cost: f64, cost_known: bool).

## Consequences

- All LLM call paths now have unified, globally-visible token accounting
- Cache tokens now flow through capture pipeline and archeologist (previously dropped by parse_usage)
- RunCounters and other accounting subsystems can now access cache data via emitted events
- Breaking change: all imports of crate::recall::usage::Usage become crate::usage::Usage
- Usage distinguishes no-cost-info via None instead of cost_known flag, simplifying null-cost checks

## Open Tail

- RunCounters.apply() still needs to consume cached_tokens from llm.response events (field added to openrouter.rs events but not yet aggregated in RunCounters)

## Evidence

- transcript lines 1-207
- transcript lines 209-209
- transcript lines 532-1020

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-28-1-move-usage-from-recall-subsystem-to.json`](transcripts/2026-06-28-1-move-usage-from-recall-subsystem-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-28-1-move-usage-from-recall-subsystem-to.json`](transcripts/raw/2026-06-28-1-move-usage-from-recall-subsystem-to.json)
