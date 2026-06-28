---
type: episode-card
date: 2026-06-26
session: 151a7e32-2bff-4b31-9196-dd6060fdb411
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/151a7e32-2bff-4b31-9196-dd6060fdb411.jsonl
salience: product
status: active
subjects:
  - daemon-indexing
  - embedding-efficiency
  - cpu-perf
  - file-watcher
supersedes: []
related_claims: []
source_lines:
  - 880-921
  - 1038-1372
captured_at: 2026-06-26T07:32:22Z
---

# Episode: Daemon stops redundant embedding: content-hash skip + watch exclusions + idle-exit

## Prior State

Daemon re-embeds entire markdown corpus on every file change (no change-detection); watches project root recursively with no exclusions (re-indexes on build output, .git, node_modules churn); accumulates one daemon per project indefinitely. index_single_file does unconditional delete+reinsert+embed even when file content is unchanged.

## Trigger

Live process inspection (sample, lsof) revealed the 375% CPU embedder and long-lived 'stuck pc hook inject' processes were actually daemonized file-watchers running daemon::run_daemon, not sidecar hangs. Initial sidecar-hang diagnosis was incorrect; real root cause was daemon doing full-corpus embedding work with zero change-detection, re-embedding 350+ files on every file touch.

## Decision

Add content-hash skip in index_single_file: compute hashes of all chunks, compare to stored; skip embed if unchanged. Exclude watch events from target/, .git/, node_modules/, .proactive-context/, hidden dirs. Daemon self-exits after 6h idle instead of accumulating indefinitely.

## Consequences

- Unchanged file re-indexes are 175× faster (0.08s vs 13.94s cold); single-file changes re-embed only that file, proportionally fast.
- Watcher ignores build output, VCS dirs, and hidden dirs; eliminates spurious re-indexing on .git churn or build system output.
- Per-project daemons eventually exit instead of living forever; prevents indefinite daemon accumulation on disk.
- New unit test added for watch-filter logic; all 352 tests pass.
- 9 stale daemon.pid files removed in this session; demonstrates impact of reaping logic.

## Open Tail

*(none)*

## Evidence

- transcript lines 880-921
- transcript lines 1038-1372

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-2-daemon-stops-redundant-embedding-content-hash.json`](transcripts/2026-06-26-2-daemon-stops-redundant-embedding-content-hash.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-2-daemon-stops-redundant-embedding-content-hash.json`](transcripts/raw/2026-06-26-2-daemon-stops-redundant-embedding-content-hash.json)
