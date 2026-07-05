---
type: episode-card
date: 2026-05-29
session: d00d68d4-f98d-46b7-be4d-51610d05bf3b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/d00d68d4-f98d-46b7-be4d-51610d05bf3b.jsonl
salience: product
status: active
subjects:
  - open-questions-detection
  - transcript-format
  - capture-pipeline
supersedes: []
related_claims: []
source_lines:
  - 3281-3284
  - 3341-3350
  - 3437-3456
captured_at: 2026-06-29T11:28:14Z
---

# Episode: Open-question detection prompt input changed from char-tail to attributed whole-turns with XML stripping

## Prior State

extract_open_questions received plain_ts (the full transcript as a single string) and took the last 6000 characters as a char-based tail slice. This could cut mid-turn, lose early user statements, and include inline XML like <system-reminder> within assistant text blocks.

## Trigger

User instructed: the prompt must include what the user actually said and all text generation with attribution (User: .../Assistant: ...), minus XML tags and tool use — not just an arbitrary char slice from the end.

## Decision

Replaced the char-based tail with a proper attributed-turn builder: passes the turns Vec directly, formats as 'User: ...\n\nAssistant: ...', strips known harness XML tags (<system-reminder>, <task-notification>, <open-questions>, etc.) from each turn's text, and truncates by dropping oldest whole turns when exceeding 8000 chars.

## Consequences

- Full conversation visible to the detection model instead of an arbitrary tail slice
- No mid-sentence truncation — whole turns are dropped from the front
- Harness XML no longer leaks into the open-questions prompt
- Tool results already excluded upstream by parse_transcript (only type:'text' content blocks reach turns)

## Open Tail

*(none)*

## Evidence

- transcript lines 3281-3284
- transcript lines 3341-3350
- transcript lines 3437-3456

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-open-question-detection-prompt-input-changed.json`](transcripts/2026-05-29-2-open-question-detection-prompt-input-changed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-open-question-detection-prompt-input-changed.json`](transcripts/raw/2026-05-29-2-open-question-detection-prompt-input-changed.json)
