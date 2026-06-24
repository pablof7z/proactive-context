---
type: episode-card
date: 2026-05-29
session: bede7c40-d729-4df6-8e97-f25dca6ce66a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/bede7c40-d729-4df6-8e97-f25dca6ce66a.jsonl
salience: product
status: superseded
subjects:
  - inject-hook
  - auto-index
  - bootstrap-logic
supersedes: []
related_claims: []
source_lines:
  - 71-82
  - 84-84
  - 117-131
  - 167-174
captured_at: 2026-06-17T12:19:32Z
---

# Episode: Inject hook auto-bootstraps unindexed projects via graduated thresholds

## Prior State

The inject hook silently returned early with `return Ok(())` when no project DB existed — it was a pure read-only consumer that required manual `proactive-context init` before it could do anything in a new project.

## Trigger

User correction: 'ok, that's wrong — what I want is…' specifying three-way branching: ≤5 files → silent no-op; >5 files & ≤5000 LOC → auto-daemonize; >5 files & >5000 LOC → list files and prompt the user to run `proactive-context init`.

## Decision

Replaced the silent early-return with a graduated auto-bootstrap: `handle_no_index` scans for `.md`/`.markdown` files (gitignore-aware via WalkBuilder), then branches on count and total LOC. Small projects still no-op; mid-size projects silently fork a background daemon; large projects surface a candidate file list (up to 100) and instruct the host agent to ask the user whether to index.

## Consequences

- Inject is no longer purely read-only in the no-DB case — it can trigger daemonize, changing its side-effect profile
- New projects with sufficient markdown content will auto-index without any manual `init` step
- Large projects (>5000 LOC) get a user-facing confirmation gate mediated through Claude Code's output
- The same WalkBuilder/gitignore logic is now shared between daemon indexer and inject bootstrap

## Open Tail

*(none)*

## Evidence

- transcript lines 71-82
- transcript lines 84-84
- transcript lines 117-131
- transcript lines 167-174

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-inject-hook-auto-bootstraps-unindexed-projects.json`](transcripts/2026-05-29-1-inject-hook-auto-bootstraps-unindexed-projects.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-inject-hook-auto-bootstraps-unindexed-projects.json`](transcripts/raw/2026-05-29-1-inject-hook-auto-bootstraps-unindexed-projects.json)
