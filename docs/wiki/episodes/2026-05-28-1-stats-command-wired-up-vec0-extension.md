---
type: episode-card
date: 2026-05-28
session: 9135070a-d269-45e6-8f71-27f2ef7246af
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9135070a-d269-45e6-8f71-27f2ef7246af.jsonl
salience: root-cause
status: active
subjects:
  - stats-command
  - sqlite-vec-extension-loading
supersedes: []
related_claims: []
source_lines:
  - 1-215
captured_at: 2026-06-17T12:15:09Z
---

# Episode: Stats command wired up; vec0 extension must be loaded for all DB paths

## Prior State

index_stats existed in db.rs but had no CLI subcommand. The ensure_vec_extension() function was private and only called inside open_db(), which meant any code opening a raw Connection without going through open_db() would fail at vec0 virtual-table queries.

## Trigger

User observed stats/status was missing. After wiring it up, running proactive-context stats crashed with 'no such module: vec0' because the Stats handler opened a bare Connection without loading the sqlite-vec extension first.

## Decision

Added a Stats CLI variant calling index_stats. Made ensure_vec_extension() public so non-daemon code paths can call it before opening their own Connection.

## Consequences

- Any future CLI subcommand that opens a DB connection must call ensure_vec_extension() before rusqlite::Connection::open, or use open_db() which already does this.
- Stats command is now user-visible: proactive-context stats shows file/chunk counts and DB path.

## Open Tail

- Stats output is thin — spec calls for embedding model, daemon status, DB size, and a --watch flag for live updates.

## Evidence

- transcript lines 1-215

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-28-1-stats-command-wired-up-vec0-extension.json`](transcripts/2026-05-28-1-stats-command-wired-up-vec0-extension.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-28-1-stats-command-wired-up-vec0-extension.json`](transcripts/raw/2026-05-28-1-stats-command-wired-up-vec0-extension.json)
