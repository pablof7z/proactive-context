---
type: noun-entry
slug: ledger
name: "ledger"
origin: extracted
source_refs:
  - transcript:236-236
  - transcript:785-786
---

# ledger

A per-session injection ledger (JSONL at `~/.proactive-context/projects/<proj>/ledger/<session_id>.jsonl`) that models what is already in Claude's live context — every prior injected briefing is still visible as a persisted system-reminder; the dedup test is whether a fact is already visible to Claude from a prior injection.
