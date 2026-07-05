---
type: episode-card
date: 2026-06-06
session: 9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/9c66cf85-c0e0-4e36-aa6f-c802d8cb0efd.jsonl
salience: product
status: superseded
subjects:
  - archeologist-tui
  - feed-rendering
  - capture-lifecycle-narration
supersedes: []
related_claims: []
source_lines:
  - 56-133
  - 137-145
captured_at: 2026-06-29T12:17:25Z
---

# Episode: Archeologist TUI narrates capture lifecycle during silent LLM calls

## Prior State

The TUI feed only rendered wiki.* mutation events and triage-skips. During multi-minute single-session captures, the dominant cost — a slow EXTRACT LLM call — produced zero feed updates, and the 'current' region just said '(mining the past)'. Interrupted sessions that reached capture.start but not capture.done were mislabeled as 'too-short'.

## Trigger

User ran archeologist for 2m17s, saw '0 captured / 1 seen (1 too-short)' with no visible activity: 'in all those 2m I didn't see a single log update, I had no idea what the thing was doing'. Investigation confirmed feed_line_for_event had no arms for capture.* or llm.* events, and the recording showed a 130s session dominated by a single silent EXTRACT call.

## Decision

Render capture.* lifecycle events (capture.start, capture.extract, capture.authority_tagging, capture.route, capture.agent_done, capture.done) as feed lines. Add live stage tracking to CurrentSession (stage string + waiting_on_model flag) driven by llm.request/llm.response events, so the 'current' region narrates 'extracting claims', 'reconciling guides', '· waiting on model' through silent LLM calls. Fix interrupted-session detection: a session that reached capture.start but not capture.done is 'interrupted', not 'too-short'.

## Consequences

- Feed now narrates the full per-session pipeline: triage → extract → route → per-guide reconcile → done
- 'Waiting on model' indicator stays visible through multi-minute LLM calls instead of a dead screen
- Interrupted sessions are correctly distinguished from genuinely too-short ones in the summary counters
- All required events already existed in events.jsonl — no capture.rs changes needed for the narration itself

## Open Tail

- Live TUI run not verified (interactive, spends real LLM budget); logic and event ordering confirmed from recordings only

## Evidence

- transcript lines 56-133
- transcript lines 137-145

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-archeologist-tui-narrates-capture-lifecycle-during.json`](transcripts/2026-06-06-1-archeologist-tui-narrates-capture-lifecycle-during.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-archeologist-tui-narrates-capture-lifecycle-during.json`](transcripts/raw/2026-06-06-1-archeologist-tui-narrates-capture-lifecycle-during.json)
