---
type: episode-card
date: 2026-05-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
salience: root-cause
status: active
subjects:
  - inject-pipeline
  - wiki-content-types
  - noun-grounding
supersedes:
  - 2026-05-29-2-noun-grounding-gap-is-missing-content
related_claims: []
source_lines:
  - 1-117
captured_at: 2026-06-17T12:49:50Z
---

# Episode: Noun-grounding is a content-gap, not a retrieval-gap

## Prior State

The inject pipeline could resolve project-specific nouns mentioned by users by retrieving relevant guides from the wiki corpus.

## Trigger

User suggested noun-grounding in inject; investigation revealed the wiki contains only principle/decision guides (e.g. inject-is-librarian-not-answerer), no entity/component definitions (e.g. 'what is the Tail TUI'). No guide defines 'the catalog' or 'the tail tui'.

## Decision

Noun-grounding is blocked on content, not retrieval. The wiki must gain entity/component guides before inject can resolve nouns. Dual-bar selection in the fast model is architecturally sound but premature until that content exists.

## Consequences

- Don't modify SELECT_PREAMBLE yet — it would tune retrieval for content that isn't there
- Layer 1 (capture producing 'what is X' guides) must precede Layer 2 (grounding-bar in select)
- Markdown-only catalog means code-only nouns (e.g. 'tail tui') can't be grounded without a separate symbol-indexing mechanism

## Open Tail

- How to add entity/component guide capture — a distiller output type change

## Evidence

- transcript lines 1-117

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-noun-grounding-is-a-content-gap.json`](transcripts/2026-05-29-1-noun-grounding-is-a-content-gap.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-noun-grounding-is-a-content-gap.json`](transcripts/raw/2026-05-29-1-noun-grounding-is-a-content-gap.json)
