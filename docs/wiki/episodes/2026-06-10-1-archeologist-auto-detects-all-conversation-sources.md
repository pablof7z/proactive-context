---
type: episode-card
date: 2026-06-10
session: 08870c09-c42d-44bf-9272-6f306cee3b52
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/08870c09-c42d-44bf-9272-6f306cee3b52.jsonl
salience: reversal
status: active
subjects:
  - archeologist-source-discovery
  - codex-integration
  - opencode-integration
supersedes: []
related_claims: []
source_lines:
  - 1078-1224
captured_at: 2026-06-17T14:07:02Z
---

# Episode: Archeologist auto-detects all conversation sources instead of opt-in flags

## Prior State

Non-Claude-Code conversation sources required explicit CLI flags (--tenex, --codex, --opencode) to be included in archeologist scans. Claude Code was the only default source.

## Trigger

User directive: 'it shouldn't take --codex and --opencode — instead it should try to load from all known paths at the same time, so if I run `pc archeologist` it should try to detect claude code, tenex, opencode, codex conversations and show their source on the selection tui'

## Decision

Removed --tenex, --codex, and --opencode CLI flags entirely. All four sources (Claude Code, TENEX, Codex, opencode) are now auto-detected by probing their default data paths on every run. Sources whose paths don't exist are skipped silently; found sources print a count line. Project display names get [tenex], [codex], [opencode] tags to indicate provenance.

## Consequences

- New codex.rs module scans ~/.codex/sessions/ and ~/.codex/archived_sessions/ for rollout-*.jsonl files; 441 legacy .json files (no cwd field) are counted and skipped
- New opencode.rs module reads ~/.local/share/opencode/opencode.db (SQLite), synthesizing flat JSONL from message+part tables with a preamble
- CLI is simpler — no source-selection flags needed; just `pc archeologist`
- Uninstalled agents are silently ignored rather than producing errors
- Source tags appear in the TUI picker so users can see which agent produced each project's sessions

## Open Tail

*(none)*

## Evidence

- transcript lines 1078-1224

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-10-1-archeologist-auto-detects-all-conversation-sources.json`](transcripts/2026-06-10-1-archeologist-auto-detects-all-conversation-sources.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-10-1-archeologist-auto-detects-all-conversation-sources.json`](transcripts/raw/2026-06-10-1-archeologist-auto-detects-all-conversation-sources.json)
