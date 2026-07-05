---
type: episode-card
date: 2026-06-06
session: 6e1a8676-e6b4-414c-b844-fbc3dbe437c0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/6e1a8676-e6b4-414c-b844-fbc3dbe437c0.jsonl
salience: root-cause
status: active
subjects:
  - capture-pipeline
  - mark-captured
  - wiki-agent-timeout
supersedes: []
related_claims: []
source_lines:
  - 297-315
  - 236-275
captured_at: 2026-06-29T12:38:24Z
---

# Episode: Unconditional mark_captured prevents retry on timeout/error

## Prior State

Code comment and known-bugs.md assumed that placing mark_captured AFTER the agent loop would allow failed sessions (timeout, API error) to be retried on a subsequent run. The design intent was that a failed agent run would NOT mark the session as captured, leaving it available for re-capture.

## Trigger

User noticed wiki was sparse despite many conversations. Investigation found 7 wiki.agent 'timeout after 300s' errors on the richest sessions. Code inspection revealed that timeout and API-error branches are caught (no early return), so execution falls through to an unconditional mark_captured_in call — permanently marking timed-out sessions as captured.

## Decision

Root-cause diagnosis accepted: the mark_captured-on-failure bug means the 300s timeout cap silently drops the densest sessions AND prevents automatic recovery on re-run. To recover, captured-session markers for those 7 sessions must be cleared (pc archeologist reset) and re-run, ideally after bumping the 300s timeout. No code fix was applied yet — assistant offered options (bump timeout, fix mark_captured to skip on failure) but user pivoted to the open-questions issue.

## Consequences

- Timed-out sessions are permanently suppressed from re-capture on plain re-run — the richest conversations are silently lost
- known-bugs.md entry stating mark runs before the loop is factually wrong — it runs after, but unconditionally, producing the same practical effect
- Recovery requires manual marker reset, not just a re-run
- The 300s timeout cap selectively bites the longest/densest sessions, creating a bias against high-value captures

## Open Tail

- Fix mark_captured to skip marking on timeout/error branches so retries happen automatically
- Bump the 300s timeout for the wiki agent loop
- Reset and re-run the 7 timed-out sessions

## Evidence

- transcript lines 297-315
- transcript lines 236-275

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-unconditional-mark-captured-prevents-retry-on.json`](transcripts/2026-06-06-1-unconditional-mark-captured-prevents-retry-on.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-unconditional-mark-captured-prevents-retry-on.json`](transcripts/raw/2026-06-06-1-unconditional-mark-captured-prevents-retry-on.json)
