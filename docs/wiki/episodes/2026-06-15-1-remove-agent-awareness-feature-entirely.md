---
type: episode-card
date: 2026-06-15
session: 9795bae3-4107-415d-89e8-ab9febbf0c71
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9795bae3-4107-415d-89e8-ab9febbf0c71.jsonl
salience: reversal
status: active
subjects:
  - agent-awareness
  - cross-agent-awareness
supersedes:
  - 2026-05-29-1-ambient-cross-agent-awareness-with-delta
related_claims: []
source_lines:
  - 1-1
  - 231-250
  - 274-291
  - 308-470
  - 491-503
captured_at: 2026-06-17T14:12:09Z
---

# Episode: Remove agent-awareness feature entirely

## Prior State

The agent-awareness feature (v0.1) was a fully wired cross-agent awareness system: a 643-line awareness.rs module, Awareness/Agents CLI subcommands, four awareness_* config fields (enabled, model, inject_min_interval_secs, expiry_secs), a product-spec doc, a validation script, and a Codex PostToolUse hook that piped agent deltas into peer sessions.

## Trigger

User directive: 'awareness of other agents has been moved -- we no longer need any of that stuff' (line 1)

## Decision

Remove the entire agent-awareness feature from the codebase, CLI, config schema, harness wiring, and all external agent configs (Codex, proactive-context). No replacement feature was introduced.

## Consequences

- 1017 lines deleted across 5 files; awareness.rs, agent-awareness.md, and validate-awareness.sh fully removed
- Awareness and Agents subcommands removed from main.rs; awareness module declaration removed
- All awareness_* config fields and their defaults removed from config.rs and config.json
- Codex PostToolUse hook block running 'pc awareness' removed from config.toml
- Harness install.rs rewritten to drop awareness wiring and introduce a new 'pc hook' subcommand instead
- Project memory file updated to mark the feature as removed

## Open Tail

- The commit message mentions 'introduce pc hook subcommand' as part of the harness refactor — whether this replaces any awareness-related wiring or is an independent change is unclear

## Evidence

- transcript lines 1-1
- transcript lines 231-250
- transcript lines 274-291
- transcript lines 308-470
- transcript lines 491-503

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-15-1-remove-agent-awareness-feature-entirely.json`](transcripts/2026-06-15-1-remove-agent-awareness-feature-entirely.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-15-1-remove-agent-awareness-feature-entirely.json`](transcripts/raw/2026-06-15-1-remove-agent-awareness-feature-entirely.json)
