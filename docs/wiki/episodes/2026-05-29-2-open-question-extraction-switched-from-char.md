---
type: episode-card
date: 2026-05-29
session: d00d68d4-f98d-46b7-be4d-51610d05bf3b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/d00d68d4-f98d-46b7-be4d-51610d05bf3b.jsonl
salience: product
status: active
subjects:
  - open-question-extraction
  - transcript-quality
supersedes: []
related_claims: []
source_lines:
  - 3281-3456
captured_at: 2026-06-17T12:56:38Z
---

# Episode: Open-question extraction switched from char-tail to attributed whole-turn transcript

## Prior State

The open-question detection prompt used `plain_ts` — a plain-text slice of the last 6000 characters of the transcript — which could cut mid-turn, lacked User/Assistant attribution, and included XML harness tags like <system-reminder>.

## Trigger

User directive: "it shouldn't just be the 6000 char tail; it MUST include what the user actually said and all the text generation (minus any <system-reminder> thing or any other xml and without tool use — and it should have it with attribution (i.e. User: ....., Assistant: ....)"

## Decision

Replaced char-based tail with `build_transcript_string(turns)` producing `User:`/`Assistant:` attributed text. Added XML-stripping regex removing `<system-reminder>`, `<task-notification>`, `<open-questions>`, and other harness tags. Truncation now drops oldest whole turns when exceeding 8000 chars instead of slicing mid-sentence. Tool results already excluded upstream by `parse_transcript` (only `type:"text"` content blocks).

## Consequences

- Detection model sees full attributed conversation with speaker labels
- No mid-turn truncation — always drops complete oldest turns
- Harness XML noise removed from the prompt
- Increased prompt size from 6000 to 8000 chars to accommodate attribution overhead

## Open Tail

*(none)*

## Evidence

- transcript lines 3281-3456

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-open-question-extraction-switched-from-char.json`](transcripts/2026-05-29-2-open-question-extraction-switched-from-char.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-open-question-extraction-switched-from-char.json`](transcripts/raw/2026-05-29-2-open-question-extraction-switched-from-char.json)
