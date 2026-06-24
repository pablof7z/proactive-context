---
type: episode-card
date: 2026-05-29
session: a94806b5-fc73-42cf-8bd2-e93aad8dabd2
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/a94806b5-fc73-42cf-8bd2-e93aad8dabd2.jsonl
salience: product
status: active
subjects:
  - tui-error-display
  - truncation-budget
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 63-67
captured_at: 2026-06-17T12:39:17Z
---

# Episode: TUI error message truncation budget increased from 60 to 250 chars

## Prior State

Error messages in the tail TUI list view were truncated at 60 characters, making them unreadable (e.g. 'CompletionError: HttpError: I…').

## Trigger

User reported they could see an error existed but could not read what it was because of aggressive truncation.

## Decision

Increased the character budget for error message display in the TUI from 60 to 250 characters before truncation.

## Consequences

- Error messages in the TUI list view now display substantially more detail, making diagnostic information visible to the user.

## Open Tail

- 250 chars may still be insufficient for very long error chains; no adaptive/smart truncation logic was introduced.

## Evidence

- transcript lines 1-1
- transcript lines 63-67

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-tui-error-message-truncation-budget-increased.json`](transcripts/2026-05-29-1-tui-error-message-truncation-budget-increased.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-tui-error-message-truncation-budget-increased.json`](transcripts/raw/2026-05-29-1-tui-error-message-truncation-budget-increased.json)
