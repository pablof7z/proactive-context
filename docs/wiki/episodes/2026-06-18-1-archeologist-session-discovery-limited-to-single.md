---
type: episode-card
date: 2026-06-18
session: 2d1210d9-f831-4b38-a5a9-abe383225f70
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/2d1210d9-f831-4b38-a5a9-abe383225f70.jsonl
salience: architecture
status: active
subjects:
  - archeologist-session-discovery
  - cwd-scoping
supersedes: []
related_claims: []
source_lines:
  - 137-208
captured_at: 2026-06-18T20:43:53Z
---

# Episode: Archeologist Session Discovery Limited to Single CWD Key

## Prior State

Archeologist was assumed to discover all historical project sessions regardless of which directory they were originally conducted in

## Trigger

User observed many early sessions missing from episode transcripts — conversations that happened before the repo existed or in alternate checkouts were absent

## Decision

Diagnosed that archeologist groups sessions by normalize_path(cwd), scanning only the ~/.claude key matching the current checkout; sessions from before the repo was created or from ~/Work/proactive-context are invisible to capture for this wiki

## Consequences

- Early design/spec conversations and pre-repo brainstorming are absent from the wiki
- Recovery requires running archeologist with --project pointed at alternate directory keys
- Any project with multiple checkouts or pre-repo history has structurally incomplete coverage

## Open Tail

- Whether to fold alternate-directory sessions into the main wiki or keep them separate
- Whether archeologist should auto-discover all project-related ~/.claude keys

## Evidence

- transcript lines 137-208

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-archeologist-session-discovery-limited-to-single.json`](transcripts/2026-06-18-1-archeologist-session-discovery-limited-to-single.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-archeologist-session-discovery-limited-to-single.json`](transcripts/raw/2026-06-18-1-archeologist-session-discovery-limited-to-single.json)
