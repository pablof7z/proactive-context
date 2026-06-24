---
type: episode-card
date: 2026-06-06
session: 9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd.jsonl
salience: product
status: active
subjects:
  - archeologist-tui
  - capture-lifecycle
  - current-session-stage
  - interrupted-sessions
supersedes: []
related_claims: []
source_lines:
  - 98-133
  - 141-145
captured_at: 2026-06-17T13:23:58Z
---

# Episode: TUI silent during multi-minute LLM calls — progress narration + interrupted-session labeling

## Prior State

The TUI feed only rendered wiki.* mutation events. During the dominant cost of a session (a single slow EXTRACT LLM call taking 2+ minutes), the screen was dead silent — the "current" region showed only "(mining the past)". Sessions that crashed mid-EXTRACT (reached capture.start but not capture.done) were mislabeled as "too-short" in the counter summary.

## Trigger

User: "in all those 2m I didn't see a single log update, I had no idea what the thing was doing." Recording analysis confirmed the event sequence: capture.start → [2+ min silent LLM call] → capture.extract → capture.authority_tagging → capture.route → wiki.* mutations.

## Decision

Wire capture lifecycle events (capture.start, capture.extract, capture.authority_tagging, capture.route, capture.agent_done, capture.done, llm.request, llm.response) into the feed and the "current" region display. Add stage tracking to CurrentSession with human-readable phase labels ("extracting claims", "reconciling guides") and a waiting_on_model flag. Add an interrupted() counter that distinguishes sessions reaching capture.start but not capture.done from genuinely too-short sessions.

## Consequences

- The "current" region now narrates live pipeline phase and shows "· waiting on model" during LLM calls
- Interrupted sessions are labeled "interrupted" rather than "too-short" in the summary counter
- Feed shows capture.* and llm.* events, not just wiki.* mutations

## Open Tail

*(none)*

## Evidence

- transcript lines 98-133
- transcript lines 141-145

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-2-tui-silent-during-multi-minute-llm.json`](transcripts/2026-06-06-2-tui-silent-during-multi-minute-llm.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-2-tui-silent-during-multi-minute-llm.json`](transcripts/raw/2026-06-06-2-tui-silent-during-multi-minute-llm.json)
