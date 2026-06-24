---
type: episode-card
date: 2026-05-29
session: 1a347f6b-c39e-4467-8ba9-a07abe0f053c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/1a347f6b-c39e-4467-8ba9-a07abe0f053c.jsonl
salience: product
status: active
subjects:
  - archeologist
  - capture
  - wiki
  - transcript
supersedes: []
related_claims: []
source_lines:
  - 1-311
  - 330-357
  - 429-557
captured_at: 2026-06-17T12:35:32Z
---

# Episode: Archeologist: bulk-historical capture over ~/.claude backlog

## Prior State

Wiki only grows from sessions that happen after installation — past conversations in ~/.claude are invisible, forcing every new install to cold-start from scratch.

## Trigger

User proposed a command that retroactively mines the entire ~/.claude/projects/**/*.jsonl backlog to pre-populate the wiki, carrying forward project lessons and user lessons.

## Decision

Build `archeologist` as a bulk-historical capture driver that replays per-session capture over the entire backlog, chronologically within each project, with: (1) an interactive crossterm multiselect project picker whose columns are free-filesystem signals only (no LLM calls before selection); (2) transcript-dated `verified:` stamps threaded via `CaptureInput.today_override` instead of `today()`; (3) periodic structural-maintenance checkpoints (`--synth-every K`) instead of evidence-free re-synthesis; (4) a live ratatui run-view dashboard. Implementation merged to master as commit 5217614.

## Consequences

- Chronological order is non-negotiable: out-of-order replay lets old reversed decisions win over corrections; sessions sorted by first-message timestamp within each project.
- Triage is an LLM call, so the picker cannot pre-triage 4,620 transcripts — columns must be free filesystem estimates (~session count, ~size, ~cost).
- v0.4's wiki-mutating tools hard-require transcript line-range evidence, making evidence-free re-synthesis impossible by construction; npm→bun-style supersession happens naturally through chronological replay instead.
- CWD routing uses the real `cwd` field inside transcripts via `normalize_path`, not the lossy encoded directory name.
- Full backlog scan estimated at ~$1,820–$2,350; the picker gates cost by project selection.
- A sibling parser `parse_transcript_meta` surfaces per-message metadata (timestamp, sidechain, meta flags) without changing `parse_transcript`'s existing callers.
- Live run-view TUI is compile-validated only (non-TTY harness); needs live terminal exercise.

## Open Tail

- Run-view TUI rendering has not been exercised in a real terminal.
- Projects split across multiple cwds (subdirectories, renamed paths) appear as separate picker rows — could add git-repo-root grouping.
- `--jobs N>1` parallelism is wired but still serial.
- `transcript_message_count` does a substring scan (compact-JSONL assumption) that could be slow over very large files.

## Evidence

- transcript lines 1-311
- transcript lines 330-357
- transcript lines 429-557

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-archeologist-bulk-historical-capture-over-claude.json`](transcripts/2026-05-29-1-archeologist-bulk-historical-capture-over-claude.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-archeologist-bulk-historical-capture-over-claude.json`](transcripts/raw/2026-05-29-1-archeologist-bulk-historical-capture-over-claude.json)
