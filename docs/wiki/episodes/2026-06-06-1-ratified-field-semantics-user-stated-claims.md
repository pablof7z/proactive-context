---
type: episode-card
date: 2026-06-06
session: b38015dd-d2aa-4e83-8671-40346633a176
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/b38015dd-d2aa-4e83-8671-40346633a176.jsonl
salience: product
status: active
subjects:
  - extract-ratified-field
  - claim-authority-tagging
supersedes: []
related_claims: []
source_lines:
  - 1626-1649
  - 1668-1681
captured_at: 2026-06-29T12:52:42Z
---

# Episode: ratified field semantics: user-stated claims now true, not false

## Prior State

The `ratified` field in the EXTRACT output was set to `true` only when an assistant proposed something AND a later user turn explicitly endorsed it. For claims the user stated directly, `ratified` was considered 'irrelevant' and set to `false`. Both maximally-authoritative user-stated claims and never-endorsed agent proposals shared the same `false` value.

## Trigger

User observed that their own directly-stated claims were all showing `ratified: false` in a real transcript run and objected — if they said it, it should be ratified by them regardless of whether an assistant turn was involved. User argued the extract LLM processing their message should mark user-stated claims as ratified.

## Decision

Flipped the semantics: `ratified` is now `true` whenever the user is the authority behind the claim — either because they stated it directly, or because they explicitly endorsed an assistant proposal. Unendorsed agent proposals remain `false`. The field name stays `ratified` (not renamed to `agent_endorsed`) because the word correctly describes the new behavior.

## Consequences

- Downstream consumers of the `ratified` field now interpret `true` as 'user is the authority' rather than the narrower 'agent proposal that was endorsed'
- The extract LLM prompt instructions at lines 1413-1415 were updated to reflect the new semantics
- Existing claims that were previously `ratified: false` despite being user-stated will now be `true`, changing filtering/review behavior
- The distinction between 'user-stated' and 'agent-proposed-and-endorsed' is now collapsed into a single `true` bucket — authorship tracking (mechanical, by turn role) remains the separate mechanism for distinguishing source

## Open Tail

- Whether downstream code or wiki review tooling that relied on the old `ratified` semantics needs adjustment

## Evidence

- transcript lines 1626-1649
- transcript lines 1668-1681

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-ratified-field-semantics-user-stated-claims.json`](transcripts/2026-06-06-1-ratified-field-semantics-user-stated-claims.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-ratified-field-semantics-user-stated-claims.json`](transcripts/raw/2026-06-06-1-ratified-field-semantics-user-stated-claims.json)
