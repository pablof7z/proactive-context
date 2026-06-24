---
type: episode-card
date: 2026-05-29
session: 26c909a1-6c07-4761-bac5-6e880cd7a063
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/26c909a1-6c07-4761-bac5-6e880cd7a063.jsonl
salience: root-cause
status: superseded
subjects:
  - inject-retrieval
  - wiki-entity-guides
  - capture-content-types
supersedes: []
related_claims: []
source_lines:
  - 1-8
  - 79-117
captured_at: 2026-06-17T12:44:57Z
---

# Episode: Noun-grounding gap is missing content, not missing retrieval

## Prior State

Assumed that when the user mentions project-specific nouns ('the catalog', 'the tail tui') the inject pipeline could be improved to retrieve definitions of those nouns from the wiki.

## Trigger

Investigation of the wiki showed all 35 guides are principle/decision guides (e.g. 'inject-is-librarian-not-answerer'), not entity/component definitions. No guide defines what 'the catalog' or 'the tail tui' IS. SELECT_PREAMBLE actively suppresses grounding-by-mention, dropping definitional guides as 'marginal.'

## Decision

The injection noun-grounding problem cannot be solved by retrieval tuning alone because the content doesn't exist. The capture side must produce entity/component guides ('what is X') before a dual-bar retrieval approach can work. The recommended first move is adding entity-guide output to the capture/distill pipeline, not tweaking SELECT_PREAMBLE.

## Consequences

- Dual-bar selection (task bar + grounding bar) is valid architecturally but blocked on content that doesn't exist yet.
- The catalog is markdown-only — code-only nouns (like 'tail tui') would need symbol indexing, not just markdown.
- This finding reframes the injection cold-start problem as a capture-side content gap, not a retrieval-side tuning problem.

## Open Tail

- Entity/component guide capture not yet designed or implemented.
- Symbol indexing for code-only nouns is a separate, larger lever not yet scoped.

## Evidence

- transcript lines 1-8
- transcript lines 79-117

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-2-noun-grounding-gap-is-missing-content.json`](transcripts/2026-05-29-2-noun-grounding-gap-is-missing-content.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-2-noun-grounding-gap-is-missing-content.json`](transcripts/raw/2026-05-29-2-noun-grounding-gap-is-missing-content.json)
