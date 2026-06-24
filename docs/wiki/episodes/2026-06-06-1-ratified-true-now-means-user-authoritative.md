---
type: episode-card
date: 2026-06-06
session: b38015dd-d2aa-4e83-8671-40346633a176
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-src-proactive-context/b38015dd-d2aa-4e83-8671-40346633a176.jsonl
salience: product
status: active
subjects:
  - extract-ratified-semantics
  - claim-authority-tagging
supersedes: []
related_claims: []
source_lines:
  - 1626-1681
captured_at: 2026-06-17T13:41:33Z
---

# Episode: ratified=true now means user-authoritative, not just agent-endorsed

## Prior State

The `ratified` field in extracted claims was `true` only when the assistant proposed something and a later user turn explicitly endorsed it. User-stated claims received `ratified: false`, conflating 'maximally authoritative' with 'not ratified' — the same value as unendorsed agent proposals.

## Trigger

User observed that claims they stated directly (e.g. NIP spec facts they pasted) were marked `ratified: false` and argued that user-stated claims should be `true` because the user is the authority, regardless of whether an agent proposed them.

## Decision

Changed `ratified` semantics so it is `true` whenever the user is the authority behind a claim — either because the user stated it directly, or because the user explicitly endorsed an assistant proposal. The prompt was updated and the implementation was changed accordingly.

## Consequences

- Previously admitted claims where the user was the source but ratification was marked false will now be classified as ratified=true, changing downstream filtering/review behavior
- The field name `ratified` now aligns with its intuitive meaning: 'user-blessed'
- Unendorsed agent proposals remain the only case for `ratified: false`

## Open Tail

*(none)*

## Evidence

- transcript lines 1626-1681

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-06-1-ratified-true-now-means-user-authoritative.json`](transcripts/2026-06-06-1-ratified-true-now-means-user-authoritative.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-06-1-ratified-true-now-means-user-authoritative.json`](transcripts/raw/2026-06-06-1-ratified-true-now-means-user-authoritative.json)
