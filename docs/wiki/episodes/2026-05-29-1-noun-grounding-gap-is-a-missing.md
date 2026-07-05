---
type: episode-card
date: 2026-05-29
session: be9ee788-301d-4758-9260-69dce3ae35b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/be9ee788-301d-4758-9260-69dce3ae35b9.jsonl
salience: root-cause
status: active
subjects:
  - inject-noun-grounding
  - wiki-entity-guides
  - select-preamble
supersedes: []
related_claims: []
source_lines:
  - 1-117
captured_at: 2026-06-29T11:12:26Z
---

# Episode: Noun-grounding gap is a missing-content problem, not a retrieval problem

## Prior State

The wiki contained only principle/decision guides (inject-is-librarian-not-answerer, config-state-drift, etc.). No entity/component definition guides existed. The injection select prompt (SELECT_PREAMBLE) was tuned strictly for task-relevance and actively suppressed grounding-by-mention.

## Trigger

User observed that cold-start prompts referencing project nouns ("the catalog", "the tail tui") would fail because Claude has no grounding for those terms. Proposed using existing docs to resolve nouns mentioned in the ask.

## Decision

Diagnosed the root cause as a missing-content problem wearing a retrieval-problem costume: the wiki has no guides that define what components ARE, only what was decided ABOUT them. No prompt tweak can ground a noun against a corpus that never defines it. The fix is two layers: (1) capture must start producing entity/component guides, (2) dual-bar SELECT_PREAMBLE (strict task bar + looser grounding bar for project-specific noun definitions) becomes valid only once entity guides exist. Do not touch the select prompt until content exists.

## Consequences

- Select prompt tuning is explicitly deferred until entity/component guides are captured
- Capture/distill pipeline needs a new output type: 'what is X' guides alongside existing 'we decided Y about X' guides
- Dual-bar selection (same fast-model call, no extra round-trip) is the proposed retrieval architecture once content exists
- Catalog is markdown-only — code-only nouns with no doc cannot be grounded without indexing symbols (a third, bigger lever)
- Grounding is most valuable at cold start where conversation context is empty

## Open Tail

- Whether capture should start producing entity/component guides (decision offered to user, not yet made)
- Dual-bar SELECT_PREAMBLE prototype (option 2, not yet started)
- Symbol indexing for code-only nouns (third lever, deferred)

## Evidence

- transcript lines 1-117

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-05-29-1-noun-grounding-gap-is-a-missing.json`](transcripts/2026-05-29-1-noun-grounding-gap-is-a-missing.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-05-29-1-noun-grounding-gap-is-a-missing.json`](transcripts/raw/2026-05-29-1-noun-grounding-gap-is-a-missing.json)
