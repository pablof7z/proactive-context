---
type: episode-card
date: 2026-05-30
session: 4b0b4989-b797-48dc-a7e6-b304b2168c57
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/4b0b4989-b797-48dc-a7e6-b304b2168c57.jsonl
salience: product
status: active
subjects:
  - inject-compile-role
  - pc-inject-prompt
supersedes:
  - 2026-05-29-1-inject-compile-model-becomes-verbatim-librarian
related_claims: []
source_lines:
  - 1-7
  - 106-116
captured_at: 2026-06-17T13:07:50Z
---

# Episode: Inject compile must be librarian not analyst — prompt loophole closed

## Prior State

The COMPILE_PREAMBLE instructed the inject model not to answer the query, but the prohibition was too vague — the model exploited loopholes by generating hypotheses ('Why it might still not render — two hypotheses'), drawing conclusions ('Bottom line'), and writing inferential analysis instead of synthesizing cited facts from wiki sources.

## Trigger

User reported that PC inject regressed to answering questions with speculative analysis instead of collecting/synthesizing wiki information — called it a 'MASSIVE violation' of how the system should work.

## Decision

Added explicit prohibitions to COMPILE_PREAMBLE against the specific failure modes: hypotheses, 'why it might' analysis, 'bottom line' conclusions, and inferential reasoning. The model is now told it is a librarian, not an analyst — every sentence must state a fact drawn directly from a cited source, nothing more.

## Consequences

- Inject briefings should no longer contain speculative or inferential content
- Prior wiki note (inject-is-librarian-not-answerer) already documented this role but the soft prohibition proved insufficient — the prompt now enforces it with enumerated bans
- Future model regressions into 'answer mode' can be diagnosed against this explicit list of banned patterns

## Open Tail

- Whether the tighter prompt actually suppresses hypothesis-generation in practice remains to be validated

## Evidence

- transcript lines 1-7
- transcript lines 106-116

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-30-1-inject-compile-must-be-librarian-not.json`](transcripts/2026-05-30-1-inject-compile-must-be-librarian-not.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-30-1-inject-compile-must-be-librarian-not.json`](transcripts/raw/2026-05-30-1-inject-compile-must-be-librarian-not.json)
